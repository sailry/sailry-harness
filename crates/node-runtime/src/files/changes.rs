//! Multi-file change sets validate every entry before writing any file. Entries
//! then run in order through the single-file kernels and stop at the first
//! failure. This is not a cross-file transaction; nothing is rolled back.
use super::{
    Control, Scope, edit, io_error, lines, mutations, path,
    save::{Target, content},
};
use cap_fs_ext::DirExt as _;
use sailry_link::CancellationToken;
use sailry_protocol::{
    ChangeKind, ChangeStatus, ChangedFile, ErrorCode, Fault, FileChange, FileChanges,
    MAX_CHANGE_BYTES, MAX_CHANGES, MAX_EDIT_BYTES, MAX_FILE_BYTES,
};
use std::{collections::BTreeSet, io::ErrorKind, ops::Range, path::PathBuf};

#[cfg(test)]
mod tests;

/// A validated entry with the text it replaces and the text it writes.
pub(crate) struct Planned {
    pub(crate) kind: ChangeKind,
    pub(crate) path: String,
    pub(crate) destination: Option<String>,
    /// Original UTF-8 text; absent for new files, moves and binary deletions.
    pub(crate) before: Option<String>,
    /// Resulting text of a content change.
    pub(crate) after: Option<String>,
    ranges: Vec<Range<usize>>,
}

impl super::Files {
    /// Validates without writing. `checked` holds per-entry Node resource failures.
    pub(crate) async fn plan_changes(
        &self,
        root: PathBuf,
        changes: Vec<FileChange>,
        checked: Vec<Option<Fault>>,
        closed: CancellationToken,
    ) -> Result<Result<Vec<Planned>, FileChanges>, Fault> {
        shape(&changes)?;
        let cancelled = closed.child_token();
        let _guard = cancelled.clone().drop_guard();
        let worker = tokio::task::spawn_blocking(move || {
            plan(
                super::Access::Restricted.scope(&root),
                &changes,
                &checked,
                &cancelled,
            )
        });
        tokio::select! {
            _ = closed.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "file change validation was cancelled")),
            result = worker => result.map_err(|_| Fault::new(ErrorCode::Internal, "file worker failed")),
        }
    }
}

/// Count bounds belong to the whole call rather than to one entry.
pub(crate) fn shape(changes: &[FileChange]) -> Result<(), Fault> {
    if changes.is_empty() || changes.len() > MAX_CHANGES {
        return Err(Fault::invalid(format!(
            "a change set requires 1 to {MAX_CHANGES} entries"
        )));
    }
    Ok(())
}

/// Any entry failure rejects the whole set before a file is written.
pub(crate) fn plan(
    root: Scope<'_>,
    changes: &[FileChange],
    checked: &[Option<Fault>],
    cancelled: &CancellationToken,
) -> Result<Vec<Planned>, FileChanges> {
    let mut seen = BTreeSet::new();
    let mut budget = MAX_CHANGE_BYTES;
    let mut planned = Vec::with_capacity(changes.len());
    let mut failures = Vec::with_capacity(changes.len());
    for (index, change) in changes.iter().enumerate() {
        let result = match checked.get(index).cloned().flatten() {
            Some(error) => Err(error),
            None => validate(root, change, &mut seen, &mut budget, cancelled),
        };
        match result {
            Ok(entry) => {
                planned.push(entry);
                failures.push(None);
            }
            Err(error) => failures.push(Some(error)),
        }
    }
    if failures.iter().all(Option::is_none) {
        return Ok(planned);
    }
    Err(FileChanges::new(
        changes
            .iter()
            .zip(failures)
            .map(|(change, error)| ChangedFile {
                status: if error.is_some() {
                    ChangeStatus::Failed
                } else {
                    ChangeStatus::NotAttempted
                },
                error,
                ..pending(change)
            })
            .collect(),
    ))
}

/// Writes validated entries in order; each write rechecks its own file revision.
pub(crate) fn apply(root: Scope<'_>, changes: &[FileChange], planned: Vec<Planned>) -> FileChanges {
    let mut stopped = false;
    let mut files = Vec::with_capacity(changes.len());
    for (change, entry) in changes.iter().zip(planned) {
        let mut file = pending(change);
        if !stopped && let Err(error) = write(root, change, entry, &mut file) {
            file.status = if error.code == ErrorCode::OutcomeUnknown {
                ChangeStatus::Unknown
            } else {
                ChangeStatus::Failed
            };
            file.error = Some(error);
            stopped = true;
        }
        files.push(file);
    }
    FileChanges::new(files)
}

fn pending(change: &FileChange) -> ChangedFile {
    ChangedFile {
        path: change.path().into(),
        destination: change.destination().map(str::to_owned),
        kind: change.kind(),
        status: ChangeStatus::NotAttempted,
        revision: None,
        size: None,
        excerpt: None,
        error: None,
    }
}

fn validate(
    root: Scope<'_>,
    change: &FileChange,
    seen: &mut BTreeSet<String>,
    budget: &mut usize,
    cancelled: &CancellationToken,
) -> Result<Planned, Fault> {
    for path in std::iter::once(change.path()).chain(change.destination()) {
        root.entries(path)?;
        // Case-only aliases may name one file on the execution platform.
        if !seen.insert(path.to_lowercase()) {
            return Err(Fault::invalid("each path may appear in only one change"));
        }
    }
    if change
        .expected_revision()
        .is_some_and(|expected| blake3::Hash::from_hex(expected).is_err())
    {
        return Err(Fault::invalid(
            "expected_revision must be the full file revision",
        ));
    }
    let control = Control::new(cancelled.clone());
    let mut entry = Planned {
        kind: change.kind(),
        path: change.path().into(),
        destination: change.destination().map(str::to_owned),
        before: None,
        after: None,
        ranges: Vec::new(),
    };
    match change {
        FileChange::Create { path, text } => {
            text_limit(text)?;
            absent(root, path)?;
            entry.after = Some(text.clone());
        }
        FileChange::Write {
            path,
            text,
            expected_revision,
        } => {
            text_limit(text)?;
            let before =
                lines::bounded(root, path, MAX_FILE_BYTES, &control)?.ok_or_else(|| {
                    Fault::invalid("file exceeds the whole-file write limit; edit it instead")
                })?;
            if blake3::hash(before.as_bytes()).to_hex().as_str() != expected_revision {
                return Err(changed());
            }
            entry.before = Some(before);
            entry.after = Some(text.clone());
        }
        FileChange::Edit {
            path,
            expected_revision,
            edits,
        } => {
            edit::changes(None, None, false, Some(edits.as_slice()))?;
            let before = lines::complete(root, path, &control)?;
            let (after, ranges) = edit::apply(&before, edits, expected_revision)?;
            entry.before = Some(before);
            entry.after = Some(after);
            entry.ranges = ranges;
        }
        FileChange::Move {
            path,
            destination,
            expected_revision,
        } => {
            source(root, path, expected_revision, &control)?;
            vacant_in_parent(root, destination)?;
        }
        FileChange::Delete {
            path,
            expected_revision,
        } => {
            source(root, path, expected_revision, &control)?;
            // A deleted text file keeps a diff preview; binary files have none.
            entry.before = lines::bounded(root, path, MAX_FILE_BYTES, &control)
                .ok()
                .flatten();
        }
    }
    let size =
        entry.before.as_ref().map_or(0, String::len) + entry.after.as_ref().map_or(0, String::len);
    *budget = budget
        .checked_sub(size)
        .ok_or_else(|| Fault::invalid("change set exceeds the combined text limit"))?;
    Ok(entry)
}

fn write(
    root: Scope<'_>,
    change: &FileChange,
    entry: Planned,
    file: &mut ChangedFile,
) -> Result<(), Fault> {
    let (expected, limit) = match change {
        FileChange::Create { path, .. } => {
            path::create_parents(root, path)?;
            (None, MAX_FILE_BYTES)
        }
        FileChange::Write {
            expected_revision, ..
        } => (Some(expected_revision.as_str()), MAX_FILE_BYTES),
        FileChange::Edit {
            expected_revision, ..
        } => (Some(expected_revision.as_str()), MAX_EDIT_BYTES),
        FileChange::Move {
            path,
            destination,
            expected_revision,
        } => {
            mutations::move_file(root, path, destination, expected_revision)?;
            file.status = ChangeStatus::Applied;
            file.revision = Some(expected_revision.clone());
            return Ok(());
        }
        FileChange::Delete {
            path,
            expected_revision,
        } => {
            mutations::delete(root, path, expected_revision)?;
            file.status = ChangeStatus::Applied;
            return Ok(());
        }
    };
    let text = entry
        .after
        .ok_or_else(|| Fault::new(ErrorCode::Internal, "planned change has no text"))?;
    let revision = blake3::hash(text.as_bytes()).to_hex().to_string();
    // The retained target compares the current revision again before replacing it.
    let written = Target::prepare(root, change.path(), expected, limit as u64, None)?.write(
        text.as_bytes(),
        text.len() as u64,
        &revision,
        #[cfg(test)]
        &|_| {},
    )?;
    file.status = ChangeStatus::Applied;
    file.revision = Some(written.revision);
    file.size = Some(written.size);
    if matches!(change, FileChange::Edit { .. }) {
        file.excerpt = Some(edit::excerpt(&text, &entry.ranges));
    }
    Ok(())
}

fn text_limit(text: &str) -> Result<(), Fault> {
    if text.len() > MAX_FILE_BYTES {
        return Err(Fault::invalid("file text exceeds the 128 KiB write limit"));
    }
    if text.contains('\0') {
        return Err(Fault::invalid("file text cannot contain NUL"));
    }
    Ok(())
}

/// A source revision covers every byte, so binary files can still move or go to Trash.
fn source(root: Scope<'_>, relative: &str, expected: &str, control: &Control) -> Result<(), Fault> {
    let mut file = path::file(root, relative)?;
    let size = file.metadata().map_err(io_error)?.len();
    if content::revision(&mut file, size, Some(&control.cancelled))? != expected {
        return Err(changed());
    }
    Ok(())
}

/// Missing parents are created for new files, so they also prove absence.
fn absent(root: Scope<'_>, relative: &str) -> Result<(), Fault> {
    let parts = root.entries(relative)?;
    let (name, parents) = parts.split_last().unwrap();
    let mut dir = path::root(root.path())?;
    for parent in parents {
        dir = match dir.open_dir_nofollow(parent) {
            Ok(dir) => dir,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(io_error(error)),
        };
    }
    vacant(&dir, name)
}

/// Moves keep the single-file rule that the destination parent already exists.
fn vacant_in_parent(root: Scope<'_>, relative: &str) -> Result<(), Fault> {
    let parts = root.entries(relative)?;
    let (name, parents) = parts.split_last().unwrap();
    vacant(&path::descend(path::root(root.path())?, parents)?, name)
}

fn vacant(dir: &cap_std::fs::Dir, name: &str) -> Result<(), Fault> {
    match dir.symlink_metadata(name) {
        Ok(_) => Err(Fault::new(
            ErrorCode::Conflict,
            "an entry already exists at this path",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

fn changed() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "file changed since it was read",
    )
}
