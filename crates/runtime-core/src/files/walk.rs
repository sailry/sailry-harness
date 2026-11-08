//! Search traversal adapted from sailry-code 67ae9fa0,
//! sailry-code-secure-file/src/secure/search.rs (Apache-2.0).
use super::{Control, io_error, path};
use cap_std::fs::{Dir, Metadata};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::gitignore::Gitignore;
use sailry_protocol::{ErrorCode, Fault, FileNames, MAX_FIND_MATCHES, SearchDiagnostic};
use std::{path::Path, time::SystemTime};

pub mod ignores;

#[derive(Default)]
pub struct Report {
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
        self.notice(path, message);
    }

    fn notice(&mut self, path: &str, message: &str) {
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

pub struct Entry {
    pub path: String,
    pub modified: SystemTime,
}

pub struct Listing {
    pub entries: Vec<Entry>,
    pub report: Report,
}

pub struct Globs {
    matcher: GlobSet,
    included: Vec<bool>,
}

impl Globs {
    pub fn is_match(&self, relative: &str, scope: &str) -> bool {
        self.matched(relative, scope)
            .map(|index| self.included[index])
            .unwrap_or_else(|| !self.included.contains(&true))
    }

    fn excludes_directory(&self, relative: &str, scope: &str) -> bool {
        self.matched(relative, scope).is_some_and(|index| {
            !self.included[index] && !self.included[index + 1..].contains(&true)
        })
    }

    fn matched(&self, relative: &str, scope: &str) -> Option<usize> {
        let mut relative = relative
            .strip_prefix(scope)
            .and_then(|suffix| suffix.strip_prefix('/'))
            .unwrap_or_else(|| {
                if relative == scope {
                    relative.rsplit('/').next().unwrap()
                } else {
                    relative
                }
            });
        let mut matched = self.matcher.matches(relative).last().copied();
        while let Some((parent, _)) = relative.rsplit_once('/') {
            let excluded = self
                .matcher
                .matches(parent)
                .into_iter()
                .filter(|index| !self.included[*index])
                .max();
            matched = matched.max(excluded);
            relative = parent;
        }
        matched
    }
}

pub fn globs(patterns: &[String]) -> Result<Globs, Fault> {
    let mut matcher = GlobSetBuilder::new();
    let mut included = Vec::new();
    for pattern in patterns {
        let (include, pattern) = match pattern.strip_prefix('!') {
            Some(pattern) => (false, pattern),
            None => (true, pattern.as_str()),
        };
        if pattern.is_empty() {
            return Err(Fault::invalid("invalid file glob pattern"));
        }
        let pattern = if pattern.contains('/') {
            pattern.to_owned()
        } else {
            format!("**/{pattern}")
        };
        matcher.add(
            GlobBuilder::new(&pattern)
                .literal_separator(true)
                .backslash_escape(false)
                .build()
                .map_err(|_| Fault::invalid("invalid file glob pattern"))?,
        );
        included.push(include);
    }
    Ok(Globs {
        matcher: matcher
            .build()
            .map_err(|_| Fault::invalid("invalid file glob pattern"))?,
        included,
    })
}

pub fn find<'a>(
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
    let mut listing = collect_matching(root, scope, usize::MAX, Some(&matcher), control)?;
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

pub fn collect<'a>(
    root: impl Into<super::Scope<'a>>,
    scope: &str,
    control: &Control,
) -> Result<Listing, Fault> {
    collect_bounded(root, scope, usize::MAX, control)
}

pub fn collect_bounded<'a>(
    root: impl Into<super::Scope<'a>>,
    scope: &str,
    limit: usize,
    control: &Control,
) -> Result<Listing, Fault> {
    collect_matching(root, scope, limit, None, control)
}

fn collect_matching<'a>(
    root: impl Into<super::Scope<'a>>,
    scope: &str,
    limit: usize,
    globs: Option<&Globs>,
    control: &Control,
) -> Result<Listing, Fault> {
    let mut entries = Vec::new();
    let report = traverse(
        root,
        scope,
        globs,
        control,
        &mut |_, _, relative, metadata| {
            if entries.len() >= limit {
                return Err(Fault::new(ErrorCode::Busy, "file discovery limit reached"));
            }
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

pub fn visit<'a>(
    root: impl Into<super::Scope<'a>>,
    scope: &str,
    globs: &Globs,
    control: &Control,
    visitor: &mut impl FnMut(&Dir, &str, &str, &Metadata) -> Result<bool, Fault>,
) -> Result<Report, Fault> {
    traverse(root, scope, Some(globs), control, visitor, &mut |_, _| {
        Ok(())
    })
}

fn traverse<'a>(
    root: impl Into<super::Scope<'a>>,
    scope: &str,
    globs: Option<&Globs>,
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
            "Git metadata cannot be searched; use the Git branches tool for remote names and URLs",
        ));
    }
    let mut report = Report::default();
    let Some(mut directory) = readable(path::root(root.path()), scope, &mut report)? else {
        return Ok(report);
    };
    let mut layers = ignores::base(&directory, root.path(), control, &mut report);
    let mut overrides = Vec::new();
    let mut prefix = String::new();
    for (index, part) in parts.iter().enumerate() {
        if let Some(layer) = ignores::load(&directory, &prefix, control, &mut report) {
            layers.push(layer);
        }
        if let Some(layer) = ignores::override_rules(&directory, &prefix, control, &mut report) {
            overrides.push(layer);
        }
        let relative = if prefix.is_empty() {
            (*part).into()
        } else {
            format!("{prefix}/{part}")
        };
        let Some(metadata) = readable(
            directory.symlink_metadata(part).map_err(io_error),
            &relative,
            &mut report,
        )?
        else {
            return Ok(report);
        };
        if metadata.is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            return Err(Fault::invalid(
                "search path must be a regular file or directory",
            ));
        }
        if metadata.is_file() {
            if index + 1 != parts.len() {
                return Err(Fault::invalid("search path has a file as a parent"));
            }
            if globs.is_none_or(|globs| globs.is_match(&relative, scope)) {
                visitor(&directory, part, &relative, &metadata)?;
            }
            report.visited += 1;
            return Ok(report);
        }
        let Some(child) = readable(path::descend(directory, &[part]), &relative, &mut report)?
        else {
            return Ok(report);
        };
        directory = child;
        prefix = relative;
    }
    let result = Walker {
        access: root.access,
        globs,
        scope,
        layers,
        overrides,
        control,
        report: &mut report,
        visitor,
        enter,
    }
    .descend(&directory, &prefix, parts.len());
    match result {
        Err(error) if error.code == ErrorCode::PermissionDenied => {
            report.diagnostic(scope, &error.message);
        }
        Err(error) if matches!(error.code, ErrorCode::Cancelled | ErrorCode::Busy) => {
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

/// Called only for filesystem access after scope policy validation.
fn readable<T>(
    result: Result<T, Fault>,
    path: &str,
    report: &mut Report,
) -> Result<Option<T>, Fault> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.code == ErrorCode::PermissionDenied => {
            report.diagnostic(path, &error.message);
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

struct Walker<'a, F, D> {
    access: super::Access,
    globs: Option<&'a Globs>,
    scope: &'a str,
    layers: Vec<Gitignore>,
    overrides: Vec<Gitignore>,
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
        control.check()?;
        (self.enter)(directory, prefix)?;
        let inherited = self.layers.len();
        let inherited_overrides = self.overrides.len();
        if let Some(layer) = ignores::load(directory, prefix, control, self.report) {
            self.layers.push(layer);
        }
        if let Some(layer) = ignores::override_rules(directory, prefix, control, self.report) {
            self.overrides.push(layer);
        }
        let result = self.contents(directory, prefix, depth);
        self.layers.truncate(inherited);
        self.overrides.truncate(inherited_overrides);
        result
    }

    fn contents(&mut self, directory: &Dir, prefix: &str, depth: usize) -> Result<bool, Fault> {
        let control = self.control;
        let mut names = Vec::new();
        for entry in directory.entries().map_err(io_error)? {
            control.check()?;
            self.report.visited += 1;
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.report.diagnostic(prefix, &io_error(error).message);
                    continue;
                }
            };
            let name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => {
                    self.report.diagnostic(prefix, "entry name is not UTF-8");
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
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(error) => {
                    self.report.diagnostic(prefix, &io_error(error).message);
                    continue;
                }
            };
            let order = if kind.is_dir() {
                format!("{name}/")
            } else {
                name.clone()
            };
            names.push((order, name));
        }
        names.sort();
        for (_, name) in names {
            control.check()?;
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            path::components_with(&relative, false, self.access)?;
            let result: Result<bool, Fault> = (|| {
                let metadata = directory.symlink_metadata(&name).map_err(io_error)?;
                if metadata.is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
                    self.report.skipped += 1;
                    return Ok(true);
                }
                if ignores::excludes(
                    self.overrides.iter().rev().chain(self.layers.iter().rev()),
                    &relative,
                    metadata.is_dir(),
                ) {
                    return Ok(true);
                }
                if metadata.is_dir() {
                    if self
                        .globs
                        .is_some_and(|globs| globs.excludes_directory(&relative, self.scope))
                    {
                        return Ok(true);
                    }
                    if depth >= 64 {
                        self.report
                            .diagnostic(&relative, "directory nesting exceeds the 64-level limit");
                        return Ok(true);
                    }
                    let child = path::descend(directory.try_clone().map_err(io_error)?, &[&name])?;
                    if !self.descend(&child, &relative, depth + 1)? {
                        return Ok(false);
                    }
                } else if self
                    .globs
                    .is_none_or(|globs| globs.is_match(&relative, self.scope))
                    && !(self.visitor)(directory, &name, &relative, &metadata)?
                {
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
