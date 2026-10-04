//! Search traversal adapted from sailry-code 67ae9fa0,
//! sailry-code-secure-file/src/secure/search.rs (Apache-2.0).
use super::{Control, io_error, open_regular, path, read_bytes};
use cap_std::fs::Dir;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::{Regex, RegexBuilder};
use sailry_protocol::*;
use std::path::Path;

const MAX_ENTRIES: usize = 10_000;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_LINE: usize = 1024;

struct Search<'a> {
    matcher: Regex,
    globs: GlobSet,
    control: &'a Control,
    visited: usize,
    bytes: usize,
    result: SearchResults,
}

pub(super) fn search(
    root: &Path,
    options: &FileSearch,
    control: &Control,
) -> Result<SearchResults, Fault> {
    if options.query.is_empty()
        || options.query.len() > 4096
        || options.globs.len() > FileSearch::MAX_GLOBS
        || options.globs.iter().any(|glob| glob.len() > 4096)
    {
        return Err(invalid());
    }
    let pattern = if options.regex {
        options.query.clone()
    } else {
        regex::escape(&options.query)
    };
    let matcher = RegexBuilder::new(&pattern)
        .case_insensitive(!options.case_sensitive)
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .map_err(|_| invalid())?;
    let mut globs = GlobSetBuilder::new();
    for pattern in &options.globs {
        globs.add(
            GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(false)
                .build()
                .map_err(|_| invalid())?,
        );
    }
    let mut search = Search {
        matcher,
        globs: globs.build().map_err(|_| invalid())?,
        control,
        visited: 0,
        bytes: 0,
        result: SearchResults {
            matches: Vec::new(),
            scanned_files: 0,
            skipped: 0,
            truncated: false,
        },
    };
    let dir = path::root(root)?;
    search.directory(&dir, "", 0)?;
    control.check()?;
    Ok(search.result)
}

impl Search<'_> {
    fn full(&self) -> bool {
        self.result.matches.len() >= MAX_SEARCH_MATCHES || self.bytes >= MAX_BYTES
    }

    fn directory(&mut self, dir: &Dir, prefix: &str, depth: usize) -> Result<(), Fault> {
        self.control.check()?;
        let mut names = Vec::new();
        for entry in dir.entries().map_err(io_error)? {
            self.control.check()?;
            if self.visited == MAX_ENTRIES {
                self.result.truncated = true;
                break;
            }
            self.visited += 1;
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    self.result.skipped += 1;
                    continue;
                }
            };
            let Ok(name) = entry.file_name().into_string() else {
                self.result.skipped += 1;
                continue;
            };
            if name.eq_ignore_ascii_case(".git") {
                continue;
            }
            if path::components(&name, false).is_err() {
                self.result.skipped += 1;
                continue;
            }
            names.push(name);
        }
        names.sort();
        for name in names {
            self.control.check()?;
            if self.full() {
                self.result.truncated = true;
                break;
            }
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if path::components(&relative, false).is_err() {
                self.result.skipped += 1;
                continue;
            }
            let result: Result<(), Fault> = (|| {
                let metadata = dir.symlink_metadata(&name).map_err(io_error)?;
                if metadata.is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
                    self.result.skipped += 1;
                } else if metadata.is_dir() {
                    if depth >= 64 || relative.len() > 4096 {
                        self.result.truncated = true;
                    } else {
                        let child = path::descend(dir.try_clone().map_err(io_error)?, &[&name])?;
                        self.directory(&child, &relative, depth + 1)?;
                    }
                } else if self.globs.is_empty() || self.globs.is_match(&relative) {
                    self.file(dir, &name, relative)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                if matches!(error.code, ErrorCode::Cancelled | ErrorCode::Busy) {
                    return Err(error);
                }
                self.result.skipped += 1;
            }
        }
        Ok(())
    }

    fn file(&mut self, dir: &Dir, name: &str, relative: String) -> Result<(), Fault> {
        let mut file = open_regular(dir, name)?;
        self.result.scanned_files += 1;
        let remaining = MAX_BYTES - self.bytes;
        let mut bytes = read_bytes(&mut file, remaining + 1, self.control)?;
        let partial = bytes.len() > remaining;
        self.result.truncated |= partial;
        bytes.truncate(remaining);
        self.bytes += bytes.len();
        if bytes.contains(&0) {
            self.result.skipped += 1;
            return Ok(());
        }
        let valid = match std::str::from_utf8(&bytes) {
            Ok(text) => text.len(),
            Err(error) if partial && error.error_len().is_none() => error.valid_up_to(),
            Err(_) => {
                self.result.skipped += 1;
                return Ok(());
            }
        };
        let text = std::str::from_utf8(&bytes[..valid]).expect("validated UTF-8 prefix");
        for (position, line) in text.lines().enumerate() {
            self.control.check()?;
            if self.matcher.is_match(line) {
                let mut end = line.len().min(MAX_LINE);
                while !line.is_char_boundary(end) {
                    end -= 1;
                }
                self.result.truncated |= end < line.len();
                self.result.matches.push(SearchMatch {
                    path: relative.clone(),
                    line_number: position + 1,
                    line: line[..end].into(),
                });
                if self.result.matches.len() == MAX_SEARCH_MATCHES {
                    self.result.truncated = true;
                    break;
                }
            }
        }
        Ok(())
    }
}

fn invalid() -> Fault {
    Fault::new(
        ErrorCode::InvalidRequest,
        "invalid file search pattern or options",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_link::CancellationToken;
    use std::time::{Duration, Instant};

    fn options() -> FileSearch {
        FileSearch {
            query: "hit".into(),
            regex: false,
            case_sensitive: true,
            globs: Vec::new(),
        }
    }

    #[test]
    fn interrupts_traversal() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let control = Control {
            cancelled: CancellationToken::new(),
            deadline: Instant::now() + Duration::from_secs(5),
        };
        control.cancelled.cancel();
        assert_eq!(
            search(&root, &options(), &control).unwrap_err().code,
            ErrorCode::Cancelled
        );
        let control = Control {
            cancelled: CancellationToken::new(),
            deadline: Instant::now(),
        };
        assert_eq!(
            search(&root, &options(), &control).unwrap_err().code,
            ErrorCode::Busy
        );
    }

    #[test]
    fn reports_partial_results() {
        let root = tempfile::tempdir().unwrap();
        let content = format!("hit\n{}\nhit", "x".repeat(MAX_BYTES));
        std::fs::write(root.path().join("large.txt"), content).unwrap();
        let control = Control {
            cancelled: CancellationToken::new(),
            deadline: Instant::now() + Duration::from_secs(5),
        };
        let result = search(&root.path().canonicalize().unwrap(), &options(), &control).unwrap();
        assert!(result.truncated);
        assert_eq!(result.scanned_files, 1);
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].line_number, 1);
    }
}
