//! Search traversal adapted from sailry-code 67ae9fa0,
//! sailry-code-secure-file/src/secure/search.rs (Apache-2.0).
use super::{Control, open_regular, path, read_bytes, text, walk};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch};
use ignore::types::TypesBuilder;
use sailry_protocol::*;
use std::{
    io,
    ops::Range,
    sync::atomic::{AtomicUsize, Ordering},
};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_LINE: usize = 16 * 1024;
const WORKERS: usize = 4;

struct FileScan {
    path: String,
    count: usize,
    revision: blake3::Hash,
    rows: Vec<SearchMatch>,
}

struct RowSink<'a> {
    path: &'a str,
    control: &'a Control,
    mode: SearchMode,
    range: Range<usize>,
    count: usize,
    rows: Vec<SearchMatch>,
}

pub(super) fn search<'a>(
    root: impl Into<super::Scope<'a>>,
    options: &FileSearch,
    control: &Control,
) -> Result<SearchResults, Fault> {
    let root = root.into();
    control.check()?;
    if options.query.is_empty() || options.limit == 0 {
        return Err(invalid());
    }
    let page_limit = options.limit.min(MAX_SEARCH_MATCHES);
    let globs = walk::globs(&options.globs)?;
    let mut types = TypesBuilder::new();
    types.add_defaults();
    if let Some(kind) = &options.file_type {
        types.select(kind);
    }
    let types = types
        .build()
        .map_err(|_| Fault::invalid("unknown file type"))?;
    let mut builder = RegexMatcherBuilder::new();
    builder
        .case_insensitive(!options.case_sensitive)
        .fixed_strings(!options.regex)
        .multi_line(true)
        .dot_matches_new_line(options.multiline)
        .crlf(true)
        .line_terminator(None)
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20);
    if !options.multiline {
        builder.line_terminator(Some(b'\n'));
    }
    let matcher = builder
        .build(&options.query)
        .map_err(|error| Fault::invalid(format!("invalid file search pattern: {error}")))?;
    let mut listing = walk::collect(root, &options.path, control)?;
    listing.entries.retain(|entry| {
        (globs.is_empty() || globs.is_match(&entry.path))
            && !types.matched(&entry.path, false).is_ignore()
    });
    listing
        .entries
        .sort_by(|left, right| left.path.cmp(&right.path));
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(WORKERS);
    let outcomes = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers.min(listing.entries.len()))
            .map(|_| {
                scope.spawn(|| {
                    let mut outcomes = Vec::new();
                    let mut searcher = engine(options);
                    let mut retained = page_limit;
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(entry) = listing.entries.get(index) else {
                            break;
                        };
                        if control.check().is_err() {
                            break;
                        }
                        let outcome = inspect(
                            root,
                            &entry.path,
                            options.mode,
                            retained,
                            &matcher,
                            &mut searcher,
                            control,
                        );
                        if let Ok(Some(scan)) = &outcome {
                            retained = retained.saturating_sub(scan.rows.len());
                        }
                        outcomes.push((index, outcome));
                    }
                    outcomes
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join())
            .collect::<Vec<_>>()
    });
    let mut result = SearchResults {
        mode: options.mode,
        offset: options.offset,
        skipped: listing.report.skipped,
        partial: listing.report.partial,
        diagnostics: listing.report.diagnostics,
        ..Default::default()
    };
    let mut scans = Vec::new();
    for outcome in outcomes {
        let outcomes =
            outcome.map_err(|_| Fault::new(ErrorCode::Internal, "file search worker failed"))?;
        for (index, outcome) in outcomes {
            result.scanned_files += 1;
            match outcome {
                Ok(Some(scan)) => scans.push((index, scan)),
                Ok(None) => result.skipped += 1,
                Err(error) => {
                    result.skipped += 1;
                    result.partial |= matches!(error.code, ErrorCode::Cancelled | ErrorCode::Busy);
                    diagnose(&mut result, &listing.entries[index].path, &error.message);
                }
            }
        }
    }
    if control.check().is_err() {
        result.partial = true;
        diagnose(
            &mut result,
            &options.path,
            "scan interrupted; results are partial, narrow path before retrying",
        );
    }
    scans.sort_by_key(|(index, _)| *index);
    if result.partial && options.mode == SearchMode::Content {
        if options.offset == 0 {
            result.matches = scans
                .into_iter()
                .flat_map(|(_, scan)| scan.rows)
                .take(page_limit)
                .collect();
        } else {
            diagnose(
                &mut result,
                &options.path,
                "an incomplete scan cannot resolve this offset; narrow path before retrying",
            );
        }
        result.truncated = true;
        return Ok(result);
    }
    let mut position = 0usize;
    let mut searcher = engine(options);
    for (_, scan) in scans {
        let count = if options.mode == SearchMode::Content {
            scan.count
        } else {
            usize::from(scan.count > 0)
        };
        let end = position.saturating_add(count);
        if count > 0 && end > options.offset && position < options.offset.saturating_add(page_limit)
        {
            match options.mode {
                SearchMode::Files => result.paths.push(scan.path),
                SearchMode::Count => result.counts.push(SearchCount {
                    path: scan.path,
                    count: scan.count,
                }),
                SearchMode::Content => {
                    let range = options.offset.saturating_sub(position)
                        ..options
                            .offset
                            .saturating_add(page_limit)
                            .saturating_sub(position)
                            .min(count);
                    match read_page(root, &scan, range, &matcher, &mut searcher, control) {
                        Ok(rows) => result.matches.extend(rows),
                        Err(error) => {
                            result.skipped += 1;
                            result.partial = true;
                            diagnose(&mut result, &scan.path, &error.message);
                        }
                    }
                }
            }
        }
        position = end;
    }
    let returned = match options.mode {
        SearchMode::Content => result.matches.len(),
        SearchMode::Files => result.paths.len(),
        SearchMode::Count => result.counts.len(),
    };
    let end = options.offset.saturating_add(returned);
    result.next_offset = (!result.partial && end < position).then_some(end);
    result.truncated =
        result.partial || end < position || result.matches.iter().any(|row| row.truncated);
    Ok(result)
}

fn engine(options: &FileSearch) -> Searcher {
    let context = if options.mode == SearchMode::Content {
        options.context
    } else {
        0
    };
    SearcherBuilder::new()
        .line_number(true)
        .multi_line(options.multiline)
        .before_context(context)
        .after_context(context)
        .heap_limit(Some(MAX_BYTES))
        .bom_sniffing(false)
        .build()
}

fn bytes<'a>(
    root: impl Into<super::Scope<'a>>,
    relative: &str,
    control: &Control,
) -> Result<Vec<u8>, Fault> {
    let root = root.into();
    let parts = root.components(relative, false)?;
    let directory = path::descend(path::root(root.path())?, &parts[..parts.len() - 1])?;
    let mut file = open_regular(&directory, parts[parts.len() - 1])?;
    if file.metadata().map_err(super::io_error)?.len() > MAX_BYTES as u64 {
        return Err(Fault::invalid(
            "file exceeds the 64 MiB search limit; use a narrower path or segmented reads",
        ));
    }
    let bytes = read_bytes(&mut file, MAX_BYTES + 1, control)?;
    if bytes.len() > MAX_BYTES {
        return Err(Fault::invalid("file exceeds the 64 MiB search limit"));
    }
    Ok(bytes)
}

fn inspect<'a>(
    root: impl Into<super::Scope<'a>>,
    relative: &str,
    mode: SearchMode,
    retained: usize,
    matcher: &RegexMatcher,
    searcher: &mut Searcher,
    control: &Control,
) -> Result<Option<FileScan>, Fault> {
    let bytes = bytes(root, relative, control)?;
    let decoded = match text::decode(&bytes) {
        Ok(decoded) => decoded,
        Err(text::Error::Binary) => return Ok(None),
        Err(error) => return Err(Fault::invalid(error.to_string())),
    };
    let mut sink = RowSink {
        path: relative,
        control,
        mode,
        range: 0..if mode == SearchMode::Content {
            retained
        } else {
            0
        },
        count: 0,
        rows: Vec::new(),
    };
    searcher
        .search_slice(matcher, decoded.as_bytes(), &mut sink)
        .map_err(|error| failure(error, control))?;
    control.check()?;
    Ok(Some(FileScan {
        path: relative.into(),
        count: sink.count,
        revision: blake3::hash(&bytes),
        rows: sink.rows,
    }))
}

fn read_page<'a>(
    root: impl Into<super::Scope<'a>>,
    scan: &FileScan,
    range: Range<usize>,
    matcher: &RegexMatcher,
    searcher: &mut Searcher,
    control: &Control,
) -> Result<Vec<SearchMatch>, Fault> {
    let bytes = bytes(root, &scan.path, control)?;
    if blake3::hash(&bytes) != scan.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "file changed during search; retry the request",
        ));
    }
    let decoded = text::decode(&bytes).map_err(|error| Fault::invalid(error.to_string()))?;
    let mut sink = RowSink {
        path: &scan.path,
        control,
        mode: SearchMode::Content,
        range,
        count: 0,
        rows: Vec::new(),
    };
    searcher
        .search_slice(matcher, decoded.as_bytes(), &mut sink)
        .map_err(|error| failure(error, control))?;
    Ok(sink.rows)
}

impl RowSink<'_> {
    fn line(&mut self, bytes: &[u8], number: usize, matched: bool) -> io::Result<bool> {
        self.control
            .check()
            .map_err(|error| io::Error::new(io::ErrorKind::Interrupted, error.message))?;
        if self.range.contains(&self.count) {
            let line = std::str::from_utf8(bytes)
                .expect("validated UTF-8 input")
                .trim_end_matches('\n');
            let line = line.strip_suffix('\r').unwrap_or(line);
            let mut end = line.len().min(MAX_LINE);
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            self.rows.push(SearchMatch {
                path: self.path.into(),
                line_number: number,
                line: line[..end].into(),
                matched,
                truncated: end < line.len(),
            });
        }
        self.count += 1;
        Ok(true)
    }
}

impl Sink for RowSink<'_> {
    type Error = io::Error;
    fn matched(&mut self, _: &Searcher, found: &SinkMatch<'_>) -> io::Result<bool> {
        if self.mode == SearchMode::Files {
            self.count = 1;
            return Ok(false);
        }
        let start = found.line_number().expect("line numbering is enabled") as usize;
        for (offset, bytes) in found.lines().enumerate() {
            if !self.line(bytes, start + offset, true)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn context(&mut self, _: &Searcher, context: &SinkContext<'_>) -> io::Result<bool> {
        if self.mode == SearchMode::Content {
            self.line(
                context.bytes(),
                context.line_number().expect("line numbering is enabled") as usize,
                false,
            )
        } else {
            Ok(true)
        }
    }
}

fn failure(error: io::Error, control: &Control) -> Fault {
    control
        .check()
        .err()
        .unwrap_or_else(|| Fault::invalid(format!("file search failed: {error}")))
}

fn diagnose(result: &mut SearchResults, path: &str, message: &str) {
    if result.diagnostics.len() < 100 {
        result.diagnostics.push(SearchDiagnostic {
            path: path.into(),
            message: message.into(),
        });
    }
}

fn invalid() -> Fault {
    Fault::invalid("invalid file search pattern or options")
}

#[cfg(test)]
mod tests;
