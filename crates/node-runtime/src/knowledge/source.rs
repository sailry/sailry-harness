//! Captured source roots retain directory capabilities throughout extraction.
use crate::files;
use cap_std::fs::Dir;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, knowledge::Position};
use std::path::{Path, PathBuf};

mod ignores;
mod read;
mod scan;
#[cfg(test)]
mod tests;

pub(super) use scan::Scan;
pub(super) use scan::Watch;

pub(super) struct SourceDoc {
    pub path: String,
    pub revision: String,
    pub modified_ms: u64,
    pub stat: Stat,
    pub parts: Vec<Part>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Stat {
    pub size: u64,
    pub modified_seconds: u64,
    pub modified_nanos: u32,
}

pub(super) struct Part {
    pub title: String,
    pub text: String,
    pub position: Position,
}

pub(super) enum Entry {
    Document(SourceDoc),
    Cached {
        path: String,
    },
    Skipped {
        path: String,
        reason: String,
        excluded: bool,
    },
}

struct Target {
    directory: Dir,
    name: String,
    relative: String,
}

fn cancelled(cancel: &CancellationToken) -> Result<(), Fault> {
    if cancel.is_cancelled() {
        Err(Fault::new(
            ErrorCode::Cancelled,
            "knowledge source scan cancelled",
        ))
    } else {
        Ok(())
    }
}

fn skipped(path: String, error: impl std::fmt::Display) -> Entry {
    Entry::Skipped {
        path,
        reason: error.to_string(),
        excluded: false,
    }
}

fn excluded(path: String, reason: impl std::fmt::Display) -> Entry {
    Entry::Skipped {
        path,
        reason: reason.to_string(),
        excluded: true,
    }
}

pub(crate) fn native(path: &str) -> Result<PathBuf, Fault> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(Fault::invalid("knowledge source path must be absolute"));
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    for (alias, destination) in [("/var", "/private/var"), ("/tmp", "/private/tmp")] {
        let alias = Path::new(alias);
        if let Ok(relative) = path.strip_prefix(alias)
            && alias.is_symlink()
        {
            if alias.canonicalize().map_err(files::io_error)? != Path::new(destination) {
                return Err(Fault::invalid("unexpected system source alias"));
            }
            return Ok(Path::new(destination).join(relative));
        }
    }
    Ok(path.to_owned())
}

/// The caller resolves this root from the execution Node's captured worktree.
pub(super) fn worktree(
    root: &Path,
    relative: &str,
    cancel: CancellationToken,
) -> Result<Scan, Fault> {
    cancelled(&cancel)?;
    let relative = if relative == "." { "" } else { relative };
    let components = files::path::components(relative, true)?;
    let directory = files::path::root(root)?;
    Scan::select(directory, &components, None, cancel)
}

/// Only controller-authorized file and directory registrations use ambient roots.
pub(super) fn controller(
    source: &sailry_protocol::knowledge::Source,
    cancel: CancellationToken,
) -> Result<Scan, Fault> {
    use sailry_protocol::knowledge::Source;
    cancelled(&cancel)?;
    match source {
        Source::Directory { path } => {
            let normalized = native(path)?;
            if ignores::protected(&normalized) {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "protected directories cannot be indexed",
                ));
            }
            Scan::select(
                files::path::root(&normalized)?,
                &[],
                Some(PathBuf::from(path)),
                cancel,
            )
        }
        Source::File { path } => {
            let normalized = native(path)?;
            let name = normalized
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| Fault::invalid("knowledge source must name a file"))?;
            files::path::components(name, false)?;
            if ignores::protected(&normalized) {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "protected files cannot be indexed",
                ));
            }
            let parent = normalized
                .parent()
                .ok_or_else(|| Fault::invalid("knowledge source file has no parent"))?;
            let original = Path::new(path)
                .parent()
                .ok_or_else(|| Fault::invalid("knowledge source file has no parent"))?;
            Scan::file(
                files::path::root(parent)?,
                name,
                Some(original.to_owned()),
                cancel,
            )
        }
        _ => Err(Fault::new(
            ErrorCode::PermissionDenied,
            "controller source extraction requires a file or directory selection",
        )),
    }
}

/// Select a current descendant of an explicitly registered controller directory.
pub(super) fn within(root: &str, relative: &str, cancel: CancellationToken) -> Result<Scan, Fault> {
    cancelled(&cancel)?;
    let components = files::path::components(relative, false)?;
    let normalized = native(root)?;
    if ignores::protected(&normalized) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "protected directories cannot be indexed",
        ));
    }
    Scan::select(
        files::path::root(&normalized)?,
        &components,
        Some(PathBuf::from(root)),
        cancel,
    )
}
