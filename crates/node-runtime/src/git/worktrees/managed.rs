//! Managed checkouts copy a Git snapshot without stashing or modifying the source.
use super::super::{fault, index, open, revision};
use git2::{ApplyLocation, DiffOptions, IndexAddOption};
use sailry_protocol::{ErrorCode, Fault};
use std::path::Path;

pub(crate) fn create(
    source: &Path,
    directory: &Path,
    branch: &str,
    expected: (&str, &str),
    include_changes: bool,
    base: Option<&str>,
    id: &str,
) -> Result<String, Fault> {
    let (head, expected_index) = expected;
    let repository = open(source)?;
    // A base commit starts the checkout elsewhere, so the source snapshot is not
    // compared and its changes cannot be carried over.
    if let Some(base) = base {
        if include_changes {
            return Err(Fault::invalid(
                "a base commit cannot include source changes",
            ));
        }
        managed(directory)?;
        return checkout(source, directory, branch, base, id);
    }
    let commit = repository
        .head()
        .map_err(fault)?
        .peel_to_commit()
        .map_err(fault)?;
    if commit.id().to_string() != head || index::revision(&repository)? != expected_index {
        return Err(revision::conflict());
    }
    if repository.index().map_err(fault)?.has_conflicts() {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "resolve index conflicts before creating a worktree",
        ));
    }
    managed(directory)?;
    let changes = if include_changes {
        let base = commit.tree().map_err(fault)?;
        let mut snapshot = repository.index().map_err(fault)?;
        let staged = repository
            .find_tree(snapshot.write_tree().map_err(fault)?)
            .map_err(fault)?;
        // Capture the working files without writing the source index. Tree diffs
        // preserve paths directly, including spaces in newly added filenames.
        snapshot
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .map_err(fault)?;
        let working = repository
            .find_tree(snapshot.write_tree().map_err(fault)?)
            .map_err(fault)?;
        let working = repository
            .diff_tree_to_tree(
                Some(&base),
                Some(&working),
                Some(DiffOptions::new().show_binary(true)),
            )
            .map_err(fault)?;
        let staged = repository
            .diff_tree_to_tree(
                Some(&base),
                Some(&staged),
                Some(DiffOptions::new().show_binary(true)),
            )
            .map_err(fault)?;
        Some((working, staged))
    } else {
        None
    };
    let path = checkout(source, directory, branch, head, id)?;
    if let Some((working, staged)) = changes {
        let target = open(Path::new(&path)).map_err(|error| unknown(error.message))?;
        for (diff, location) in [
            (working, ApplyLocation::WorkDir),
            (staged, ApplyLocation::Index),
        ] {
            if diff.deltas().len() > 0 {
                target.apply(&diff, location, None).map_err(unknown)?;
            }
        }
    }
    Ok(path)
}

fn unknown(error: impl std::fmt::Display) -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        format!("worktree was created but copying changes did not complete: {error}"),
    )
}

fn managed(directory: &Path) -> Result<(), Fault> {
    let root = directory
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| Fault::invalid("managed worktree directory is unavailable"))?;
    if super::location::prepare(root)? != directory {
        return Err(Fault::invalid("managed worktree directory is invalid"));
    }
    Ok(())
}

fn checkout(
    source: &Path,
    directory: &Path,
    branch: &str,
    commit: &str,
    id: &str,
) -> Result<String, Fault> {
    let destination = directory.join(id);
    let destination = destination
        .to_str()
        .ok_or_else(|| Fault::invalid("worktree path is not UTF-8"))?;
    super::creation::create(source, destination, branch, commit, id)
}
