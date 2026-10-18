//! Search traversal adapted from sailry-code 67ae9fa0,
//! sailry-code-secure-file/src/secure/search.rs (Apache-2.0).
use super::{Control, io_error, path};
use cap_std::fs::{Dir, Metadata};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::gitignore::Gitignore;
use sailry_protocol::{ErrorCode, Fault, FileNames, MAX_FIND_MATCHES, SearchDiagnostic};
use std::{path::Path, time::SystemTime};

pub(crate) mod ignores;

#[derive(Default)]
pub(crate) struct Report {
    pub visited: usize,
    pub skipped: usize,
    pub truncated: bool,
    pub partial: bool,
    pub diagnostics: Vec<SearchDiagnostic>,
}

impl Report {
    fn diagnostic(&mut self, path: &str, message: &str) {
        self.partial = true;
        self.truncated = true;
        self.skipped += 1;
        // Diagnostics describe omitted data without allowing a large tree to
        // produce an unbounded response. The aggregate skipped count is retained.
        if self.diagnostics.len() < 100 {
            self.diagnostics.push(SearchDiagnostic {
                path: path.into(),
                message: message.into(),
            });
        }
    }
}

pub(super) struct Entry {
    pub path: String,
    pub modified: SystemTime,
}

pub(super) struct Listing {
    pub entries: Vec<Entry>,
    pub report: Report,
}

pub(super) fn globs(patterns: &[String]) -> Result<GlobSet, Fault> {
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

pub(super) fn find<'a>(
    root: impl Into<super::Scope<'a>>,
    patterns: &[String],
    scope: &str,
    limit: usize,
    offset: usize,
    control: &Control,
) -> Result<FileNames, Fault> {
    if patterns.is_empty() || patterns.iter().any(String::is_empty) || limit == 0 {
        return Err(Fault::invalid(
            "file discovery requires nonempty glob patterns and a positive limit",
        ));
    }
    let matcher = globs(patterns)?;
    let mut listing = collect(root, scope, control)?;
    listing
        .entries
        .retain(|entry| matcher.is_match(&entry.path));
    listing.entries.sort_by(|left, right| {
        right
            .modified
            .cmp(&left.modified)
            .then_with(|| left.path.cmp(&right.path))
    });
    let total = listing.entries.len();
    let paths: Vec<_> = listing
        .entries
        .into_iter()
        .skip(offset)
        .take(limit.min(MAX_FIND_MATCHES))
        .map(|entry| entry.path)
        .collect();
    let end = offset.saturating_add(paths.len());
    let next_offset = (!listing.report.partial && end < total).then_some(end);
    Ok(FileNames {
        paths,
        visited: listing.report.visited,
        skipped: listing.report.skipped,
        truncated: listing.report.partial || end < total,
        partial: listing.report.partial,
        offset,
        next_offset,
        diagnostics: listing.report.diagnostics,
    })
}

pub(super) fn collect<'a>(
    root: impl Into<super::Scope<'a>>,
    scope: &str,
    control: &Control,
) -> Result<Listing, Fault> {
    let mut entries = Vec::new();
    let report = traverse(
        root,
        scope,
        true,
        control,
        &mut |_, _, relative, metadata| {
            entries.push(Entry {
                path: relative.into(),
                modified: metadata.modified().map_err(io_error)?.into_std(),
            });
            Ok(true)
        },
        &mut |_, _| Ok(()),
    )?;
    Ok(Listing { entries, report })
}

pub(crate) fn visit(
    root: &Path,
    control: &Control,
    visitor: &mut impl FnMut(&Dir, &str, &str) -> Result<bool, Fault>,
) -> Result<Report, Fault> {
    // Existing workspace language detection keeps its own filtering policy.
    traverse(
        root,
        "",
        false,
        control,
        &mut |dir, name, relative, _| visitor(dir, name, relative),
        &mut |_, _| Ok(()),
    )
}

fn traverse<'a>(
    root: impl Into<super::Scope<'a>>,
    scope: &str,
    respect_ignores: bool,
    control: &Control,
    visitor: &mut impl FnMut(&Dir, &str, &str, &Metadata) -> Result<bool, Fault>,
    enter: &mut impl FnMut(&Dir, &str) -> Result<(), Fault>,
) -> Result<Report, Fault> {
    let root = root.into();
    control.check()?;
    let scope = if scope == "." { "" } else { scope };
    let parts = root.components(scope, true)?;
    if parts.iter().any(|part| part.eq_ignore_ascii_case(".git")) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "Git metadata cannot be searched",
        ));
    }
    let mut directory = path::root(root.path())?;
    let mut report = Report::default();
    let mut layers = Vec::new();
    let mut prefix = String::new();
    for (index, part) in parts.iter().enumerate() {
        if respect_ignores
            && let Some(layer) = ignores::load(&directory, &prefix, control, &mut report)
        {
            layers.push(layer);
        }
        let metadata = directory.symlink_metadata(part).map_err(io_error)?;
        let relative = if prefix.is_empty() {
            (*part).into()
        } else {
            format!("{prefix}/{part}")
        };
        if metadata.is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            return Err(Fault::invalid(
                "search path must be a regular file or directory",
            ));
        }
        if respect_ignores && ignores::excludes(&layers, &relative, metadata.is_dir()) {
            return Ok(report);
        }
        if metadata.is_file() {
            if index + 1 != parts.len() {
                return Err(Fault::invalid("search path has a file as a parent"));
            }
            visitor(&directory, part, &relative, &metadata)?;
            report.visited += 1;
            return Ok(report);
        }
        directory = path::descend(directory, &[part])?;
        prefix = relative;
    }
    let result = Walker {
        access: root.access,
        respect_ignores,
        layers,
        control,
        report: &mut report,
        visitor,
        enter,
    }
    .descend(&directory, &prefix, parts.len());
    match result {
        Err(error)
            if respect_ignores && matches!(error.code, ErrorCode::Cancelled | ErrorCode::Busy) =>
        {
            report.diagnostic(
                scope,
                "scan interrupted; results are partial, narrow path before retrying",
            );
        }
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    Ok(report)
}

struct Walker<'a, F, D> {
    access: super::Access,
    respect_ignores: bool,
    layers: Vec<Gitignore>,
    control: &'a Control,
    report: &'a mut Report,
    visitor: &'a mut F,
    enter: &'a mut D,
}

impl<F, D> Walker<'_, F, D>
where
    F: FnMut(&Dir, &str, &str, &Metadata) -> Result<bool, Fault>,
    D: FnMut(&Dir, &str) -> Result<(), Fault>,
{
    fn descend(&mut self, directory: &Dir, prefix: &str, depth: usize) -> Result<bool, Fault> {
        let control = self.control;
        let respect_ignores = self.respect_ignores;
        control.check()?;
        (self.enter)(directory, prefix)?;
        let inherited = self.layers.len();
        if respect_ignores
            && let Some(layer) = ignores::load(directory, prefix, control, self.report)
        {
            self.layers.push(layer);
        }
        let result = self.contents(directory, prefix, depth);
        self.layers.truncate(inherited);
        result
    }

    fn contents(&mut self, directory: &Dir, prefix: &str, depth: usize) -> Result<bool, Fault> {
        let control = self.control;
        let respect_ignores = self.respect_ignores;
        let mut names = Vec::new();
        for entry in directory.entries().map_err(io_error)? {
            control.check()?;
            self.report.visited += 1;
            let name = match entry
                .ok()
                .and_then(|entry| entry.file_name().into_string().ok())
            {
                Some(name) => name,
                None => {
                    self.report.diagnostic(
                        prefix,
                        "directory entry is unreadable or its name is not UTF-8",
                    );
                    continue;
                }
            };
            if name.eq_ignore_ascii_case(".git")
                || self.access == super::Access::Restricted && path::worktrees(prefix, &name)
            {
                continue;
            }
            if path::components(&name, false).is_err() {
                self.report.diagnostic(prefix, "unsupported entry name");
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
            let result: Result<bool, Fault> = (|| {
                let metadata = directory.symlink_metadata(&name).map_err(io_error)?;
                if metadata.is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
                    self.report.skipped += 1;
                    return Ok(true);
                }
                if respect_ignores && ignores::excludes(&self.layers, &relative, metadata.is_dir())
                {
                    return Ok(true);
                }
                if path::components_with(&relative, false, self.access).is_err() {
                    self.report.diagnostic(&relative, "unsupported entry path");
                    return Ok(true);
                }
                if metadata.is_dir() {
                    if depth >= 64 {
                        self.report
                            .diagnostic(&relative, "directory nesting exceeds the 64-level limit");
                        return Ok(true);
                    }
                    let child = path::descend(directory.try_clone().map_err(io_error)?, &[&name])?;
                    if !self.descend(&child, &relative, depth + 1)? {
                        return Ok(false);
                    }
                } else if !(self.visitor)(directory, &name, &relative, &metadata)? {
                    self.report.truncated = true;
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
                Err(error) => self.report.diagnostic(&relative, &error.message),
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
