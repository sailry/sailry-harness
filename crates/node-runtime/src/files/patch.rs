//! Codex-format patches resolve into ordinary change-set entries before approval.
//! Parsing and context matching are adapted from openai/codex (Apache-2.0); see
//! `parser` and `update`. Each update or deletion reads its target once and records
//! that full revision as the entry's expected revision, which the change-set engine
//! checks again before replacing the file. Nothing here writes.
use super::{Control, Scope, changes, lines, notebook, path};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, FileChange, MAX_CHANGES, MAX_FILE_BYTES};
use std::path::PathBuf;

mod parser;
#[cfg(test)]
mod tests;
mod update;

pub(crate) use parser::parse;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Hunk {
    Add {
        path: String,
        contents: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        destination: Option<String>,
        /// In file order: each chunk is located after the previous one.
        chunks: Vec<Chunk>,
    },
}

impl Hunk {
    /// The patched path, then any move destination.
    pub(crate) fn paths(&self) -> impl Iterator<Item = &str> {
        let (path, destination) = match self {
            Self::Add { path, .. } | Self::Delete { path } => (path, None),
            Self::Update {
                path, destination, ..
            } => (path, destination.as_deref()),
        };
        std::iter::once(path.as_str()).chain(destination)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Chunk {
    /// An `@@` line; the chunk is located after its first exact match.
    pub(crate) context: Option<String>,
    pub(crate) old: Vec<String>,
    pub(crate) new: Vec<String>,
    /// Positions in `old` and `new` of lines parsed as context.
    pub(crate) kept: Vec<(usize, usize)>,
    /// `old` must end the file.
    pub(crate) end_of_file: bool,
}

impl Chunk {
    fn is_empty(&self) -> bool {
        self.old.is_empty() && self.new.is_empty()
    }

    fn keep(&mut self, line: String) {
        self.kept.push((self.old.len(), self.new.len()));
        self.old.push(line.clone());
        self.new.push(line);
    }
}

/// One parsed hunk with the revision this session last observed for its patched
/// path and any Node resource failure found before reading.
pub(crate) struct Target {
    pub(crate) hunk: Hunk,
    pub(crate) observed: Option<String>,
    pub(crate) fault: Option<Fault>,
}

impl super::Files {
    /// Reads each target and returns its entries, each with a failure that rejects the set.
    pub(crate) async fn resolve_patch(
        &self,
        root: PathBuf,
        targets: Vec<Target>,
        closed: CancellationToken,
    ) -> Result<Vec<(FileChange, Option<Fault>)>, Fault> {
        let cancelled = closed.child_token();
        let _guard = cancelled.clone().drop_guard();
        let worker = tokio::task::spawn_blocking(move || {
            resolve(super::Access::Restricted.scope(&root), targets, &cancelled)
        });
        tokio::select! {
            _ = closed.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "patch validation was cancelled")),
            result = worker => result.map_err(|_| Fault::new(ErrorCode::Internal, "file worker failed"))?,
        }
    }
}

pub(crate) fn resolve(
    root: Scope<'_>,
    targets: Vec<Target>,
    cancelled: &CancellationToken,
) -> Result<Vec<(FileChange, Option<Fault>)>, Fault> {
    if targets.is_empty() {
        return Err(Fault::invalid("patch contains no file changes"));
    }
    let mut entries = Vec::with_capacity(targets.len());
    for target in targets {
        let control = Control::new(cancelled.clone());
        control.check()?;
        let resolved = match target.fault {
            Some(fault) => Err(fault),
            None => entry(root, &target.hunk, target.observed.as_deref(), &control),
        };
        match resolved {
            Ok(changes) => entries.extend(changes.into_iter().map(|change| (change, None))),
            Err(fault) => entries.push((placeholder(target.hunk), Some(fault))),
        }
    }
    if entries.len() > MAX_CHANGES {
        return Err(Fault::invalid(format!(
            "a patch can change at most {MAX_CHANGES} files; a moved and edited file counts twice"
        )));
    }
    Ok(entries)
}

/// A move with unchanged content keeps the file; an edited move writes the new file
/// and moves the original to Trash, so it counts as two entries.
fn entry(
    root: Scope<'_>,
    hunk: &Hunk,
    observed: Option<&str>,
    control: &Control,
) -> Result<Vec<FileChange>, Fault> {
    Ok(match hunk {
        Hunk::Add { path, contents } => vec![FileChange::Create {
            path: path.clone(),
            text: notebook::prepare_creation(path, contents)?.into_owned(),
        }],
        Hunk::Delete { path } => {
            let revision = changes::revision(root, path, control)?;
            current(observed, &revision)?;
            vec![FileChange::Delete {
                path: path.clone(),
                expected_revision: revision,
            }]
        }
        Hunk::Update {
            path,
            destination,
            chunks,
        } => {
            let before = lines::complete(root, path, control)?;
            let revision = blake3::hash(before.as_bytes()).to_hex().to_string();
            current(observed, &revision)?;
            let update = update::apply(&before, chunks)?;
            control.check()?;
            match destination {
                None => vec![content(path, &before, revision, update)?],
                Some(destination) if update.after == before && parent(root, destination) => {
                    vec![FileChange::Move {
                        path: path.clone(),
                        destination: destination.clone(),
                        expected_revision: revision,
                    }]
                }
                Some(destination) => vec![
                    FileChange::Create {
                        path: destination.clone(),
                        text: notebook::prepare_creation(destination, &update.after)?.into_owned(),
                    },
                    FileChange::Delete {
                        path: path.clone(),
                        expected_revision: revision,
                    },
                ],
            }
        }
    })
}

/// Updates and deletions require the session's latest observation to be the current file.
fn current(observed: Option<&str>, revision: &str) -> Result<(), Fault> {
    match observed {
        Some(observed) if observed == revision => Ok(()),
        Some(_) => Err(Fault::new(
            ErrorCode::RevisionConflict,
            "file changed since this session last read it; read the file again",
        )),
        None => Err(Fault::new(
            ErrorCode::RevisionConflict,
            "file has not been read in this session; read the file again",
        )),
    }
}

/// Exact edits keep numbered excerpts in the result. A whole-file write is the
/// fallback when edits cannot reproduce the patched text, within its 128 KiB limit.
fn content(
    path: &str,
    before: &str,
    revision: String,
    update: update::Update<'_>,
) -> Result<FileChange, Fault> {
    if let Some(edits) = update.edits(before, &revision) {
        return Ok(FileChange::Edit {
            path: path.into(),
            expected_revision: revision,
            edits,
        });
    }
    if before.len() > MAX_FILE_BYTES || update.after.len() > MAX_FILE_BYTES {
        return Err(Fault::invalid(
            "the patch cannot be applied exactly to this file's line endings; use edit_file",
        ));
    }
    Ok(FileChange::Write {
        path: path.into(),
        text: update.after,
        expected_revision: revision,
    })
}

fn parent(root: Scope<'_>, relative: &str) -> bool {
    root.entries(relative).is_ok_and(|parts| {
        path::root(root.path())
            .and_then(|dir| path::descend(dir, &parts[..parts.len() - 1]))
            .is_ok()
    })
}

/// Names a rejected hunk's files in the set result; it is never written.
fn placeholder(hunk: Hunk) -> FileChange {
    match hunk {
        Hunk::Add { path, contents } => FileChange::Create {
            path,
            text: contents,
        },
        Hunk::Delete { path } => FileChange::Delete {
            path,
            expected_revision: String::new(),
        },
        Hunk::Update {
            path,
            destination: None,
            ..
        } => FileChange::Edit {
            path,
            expected_revision: String::new(),
            edits: Vec::new(),
        },
        Hunk::Update {
            path,
            destination: Some(destination),
            ..
        } => FileChange::Move {
            path,
            destination,
            expected_revision: String::new(),
        },
    }
}
