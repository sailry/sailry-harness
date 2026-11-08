//! Search traversal adapted from sailry-code 67ae9fa0,
//! sailry-code-secure-file/src/secure/search.rs (Apache-2.0).
use super::{Control, open_regular, walk};
use cap_std::fs::{Dir, Metadata};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch};
use ignore::types::TypesBuilder;
use sailry_protocol::*;
use std::{io, ops::Range};

mod reader;

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_LINE: usize = 16 * 1024;

struct FileScan {
    count: usize,
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
        return Err(Fault::invalid("invalid file search pattern or options"));
    }
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
    let page_limit = options.limit.min(MAX_SEARCH_MATCHES);
    let mut result = SearchResults {
        mode: options.mode,
        offset: options.offset,
        ..Default::default()
    };
    let mut position = 0usize;
    let mut searcher = engine(options);
    let report = walk::visit(
        root,
        &options.path,
        &globs,
        control,
        &mut |directory, name, relative, metadata| {
            if types.matched(relative, false).is_ignore() {
                return Ok(true);
            }
            result.scanned_files += 1;
            let range = if options.mode == SearchMode::Content {
                options.offset.saturating_sub(position)
                    ..options
                        .offset
                        .saturating_add(page_limit)
                        .saturating_sub(position)
            } else {
                0..0
            };
            match inspect(
                directory,
                name,
                metadata,
                &matcher,
                &mut searcher,
                RowSink {
                    path: relative,
                    control,
                    mode: options.mode,
                    range,
                    count: 0,
                    rows: Vec::new(),
                },
            ) {
                Ok(Some(scan)) => {
                    let count = if options.mode == SearchMode::Content {
                        scan.count
                    } else {
                        usize::from(scan.count > 0)
                    };
                    if count > 0
                        && position.saturating_add(count) > options.offset
                        && position < options.offset.saturating_add(page_limit)
                    {
                        match options.mode {
                            SearchMode::Files => result.paths.push(relative.into()),
                            SearchMode::Count => result.counts.push(SearchCount {
                                path: relative.into(),
                                count: scan.count,
                            }),
                            SearchMode::Content => result.matches.extend(scan.rows),
                        }
                    }
                    position = position.saturating_add(count);
                }
                Ok(None) => result.skipped += 1,
                Err(error) => {
                    result.skipped += 1;
                    result.partial |= matches!(
                        error.code,
                        ErrorCode::NotFound
                            | ErrorCode::PermissionDenied
                            | ErrorCode::Cancelled
                            | ErrorCode::Busy
                    );
                    diagnose(&mut result, relative, &error.message);
                }
            }
            Ok(true)
        },
    )?;
    result.skipped += report.skipped;
    result.partial |= report.partial;
    for diagnostic in report.diagnostics {
        diagnose(&mut result, &diagnostic.path, &diagnostic.message);
    }
    if result.partial && options.mode == SearchMode::Content && options.offset > 0 {
        result.matches.clear();
        diagnose(
            &mut result,
            &options.path,
            "an incomplete scan cannot resolve this offset; narrow path before retrying",
        );
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
        .build()
}

fn inspect(
    directory: &Dir,
    name: &str,
    metadata: &Metadata,
    matcher: &RegexMatcher,
    searcher: &mut Searcher,
    mut sink: RowSink<'_>,
) -> Result<Option<FileScan>, Fault> {
    if metadata.len() > MAX_BYTES as u64 {
        return Err(Fault::invalid(
            "file exceeds the 64 MiB search limit; use a narrower path or segmented reads",
        ));
    }
    let file = open_regular(directory, name)?;
    let mut reader = reader::Text::new(file, sink.control);
    let outcome = searcher
        .search_reader(matcher, &mut reader, &mut sink)
        .and_then(|()| {
            if sink.mode == SearchMode::Files && sink.count > 0 {
                io::copy(&mut reader, &mut io::sink())?;
            }
            Ok(())
        });
    if reader.binary {
        return Ok(None);
    }
    outcome.map_err(|error| failure(error, sink.control))?;
    sink.control.check()?;
    Ok(Some(FileScan {
        count: sink.count,
        rows: sink.rows,
    }))
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

#[cfg(test)]
mod tests;
