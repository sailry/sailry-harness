//! Search traversal adapted from sailry-code 67ae9fa0,
//! sailry-code-secure-file/src/secure/search.rs (Apache-2.0).
use super::{Control, io_error, path};
use cap_std::fs::Dir;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use sailry_protocol::{ErrorCode, Fault, FileNames, FileSearch, MAX_FILE_BYTES, MAX_FIND_MATCHES};
use std::path::Path;

const MAX_ENTRIES: usize = 10_000;

#[derive(Default)]
pub(super) struct Report {
    pub visited: usize,
    pub skipped: usize,
    pub truncated: bool,
}

pub(super) fn globs(patterns: &[String]) -> Result<GlobSet, Fault> {
    if patterns.len() > FileSearch::MAX_GLOBS
        || patterns
            .iter()
            .any(|pattern| pattern.encode_utf16().take(4097).count() > 4096)
    {
        return Err(Fault::invalid("invalid file glob pattern or options"));
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(false)
                .build()
                .map_err(|_| Fault::invalid("invalid file glob pattern"))?,
        );
    }
    builder
        .build()
        .map_err(|_| Fault::invalid("invalid file glob pattern"))
}

pub(super) fn find(
    root: &Path,
    patterns: &[String],
    control: &Control,
) -> Result<FileNames, Fault> {
    if patterns.is_empty() || patterns.iter().any(String::is_empty) {
        return Err(Fault::invalid(
            "file discovery requires nonempty glob patterns",
        ));
    }
    let matcher = globs(patterns)?;
    let mut paths = Vec::new();
    let mut bytes = 0;
    let report = visit(root, control, &mut |_, _, relative| {
        if matcher.is_match(relative) {
            if paths.len() == MAX_FIND_MATCHES || bytes + relative.len() > MAX_FILE_BYTES {
                return Ok(false);
            }
            bytes += relative.len();
            paths.push(relative.to_owned());
        }
        Ok(true)
    })?;
    Ok(FileNames {
        paths,
        visited: report.visited,
        skipped: report.skipped,
        truncated: report.truncated,
    })
}

pub(super) fn visit(
    root: &Path,
    control: &Control,
    visitor: &mut impl FnMut(&Dir, &str, &str) -> Result<bool, Fault>,
) -> Result<Report, Fault> {
    let mut report = Report::default();
    directory(&path::root(root)?, "", 0, control, &mut report, visitor)?;
    control.check()?;
    Ok(report)
}

fn directory(
    directory: &Dir,
    prefix: &str,
    depth: usize,
    control: &Control,
    report: &mut Report,
    visitor: &mut impl FnMut(&Dir, &str, &str) -> Result<bool, Fault>,
) -> Result<bool, Fault> {
    control.check()?;
    let mut names = Vec::new();
    for entry in directory.entries().map_err(io_error)? {
        control.check()?;
        if report.visited == MAX_ENTRIES {
            report.truncated = true;
            break;
        }
        report.visited += 1;
        let name = match entry
            .ok()
            .and_then(|entry| entry.file_name().into_string().ok())
        {
            Some(name) => name,
            None => {
                report.skipped += 1;
                continue;
            }
        };
        if name.eq_ignore_ascii_case(".git") {
            continue;
        }
        if path::components(&name, false).is_err() {
            report.skipped += 1;
            continue;
        }
        names.push(name);
    }
    names.sort();
    for name in names {
        control.check()?;
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if path::components(&relative, false).is_err() {
            report.skipped += 1;
            continue;
        }
        let result: Result<bool, Fault> = (|| {
            let metadata = directory.symlink_metadata(&name).map_err(io_error)?;
            if metadata.is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
                report.skipped += 1;
            } else if metadata.is_dir() {
                if depth >= 64 || relative.len() > 4096 || report.visited == MAX_ENTRIES {
                    report.truncated = true;
                } else {
                    let child = path::descend(directory.try_clone().map_err(io_error)?, &[&name])?;
                    if !self::directory(&child, &relative, depth + 1, control, report, visitor)? {
                        return Ok(false);
                    }
                }
            } else if !visitor(directory, &name, &relative)? {
                report.truncated = true;
                return Ok(false);
            }
            Ok(true)
        })();
        match result {
            Ok(false) => return Ok(false),
            Ok(true) => {}
            Err(error) if matches!(error.code, ErrorCode::Cancelled | ErrorCode::Busy) => {
                return Err(error);
            }
            Err(_) => report.skipped += 1,
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
