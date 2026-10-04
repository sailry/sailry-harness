//! Safe checkout follows sailry-code 67ae9fa0 command_support.rs (Apache-2.0).
//! libgit2 owns checkout and index publication; partial effects are never replayed.
use super::{check_root, git_error, index, io_error, open, path, revision, worktrees};
use git2::{Branch, BranchType, Oid, Reference, RepositoryState, build::CheckoutBuilder};
use sailry_protocol::{ErrorCode, Fault, GitBranch};
use std::path::Path;

pub(crate) fn switch(
    root: &Path,
    name: &str,
    commit: &str,
    expected_head: Option<&str>,
    expected_branch: Option<&str>,
    expected_index: &str,
) -> Result<GitBranch, Fault> {
    if name.len() > 1024 || !Branch::name_is_valid(name).map_err(git_error)? || commit.len() != 40 {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid branch or commit identifier",
        ));
    }
    let expected_commit = Oid::from_str(commit).map_err(git_error)?;
    let expected_head = expected_head
        .map(Oid::from_str)
        .transpose()
        .map_err(git_error)?;
    let previous = expected_branch
        .map(|name| format!("refs/heads/{name}"))
        .unwrap_or_else(|| "HEAD".into());
    if !Reference::is_valid_name(&previous) {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid previous branch name",
        ));
    }
    let retained = path::root(root)?;
    let repository = open(root)?;
    if repository.state() != RepositoryState::Clean {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "finish the current Git operation before switching branches",
        ));
    }
    let target_name = format!("refs/heads/{name}");
    let mut refs = repository.transaction().map_err(git_error)?;
    refs.lock_ref("HEAD").map_err(git_error)?;
    refs.lock_ref(&target_name).map_err(git_error)?;
    if previous != "HEAD" && previous != target_name {
        refs.lock_ref(&previous).map_err(git_error)?;
    }
    revision::check(&repository, &previous, expected_head, expected_index)?;
    let target = repository
        .find_branch(name, BranchType::Local)
        .map_err(git_error)?;
    if target.get().target() != Some(expected_commit) {
        return Err(revision::conflict());
    }
    if previous == target_name {
        return Ok(GitBranch {
            name: name.into(),
            commit: expected_commit.to_string(),
            remote: false,
            current: true,
        });
    }
    worktrees::check_available(&repository, root, &target_name)?;
    let tree = target.get().peel_to_tree().map_err(git_error)?;
    check_root(&retained, root)?;
    revision::check(&repository, &previous, expected_head, expected_index)?;
    // Stage the HEAD change while it is still only in memory. Do not invent a
    // second index serializer or roll back files after a partial native checkout.
    refs.set_symbolic_target(
        "HEAD",
        &target_name,
        None,
        &format!("checkout: moving to {name}"),
    )
    .map_err(git_error)?;
    apply(&repository, &tree)?;
    check_root(&retained, root).map_err(|_| unknown())?;
    refs.commit().map_err(|_| unknown())?;
    Ok(GitBranch {
        name: name.into(),
        commit: expected_commit.to_string(),
        remote: false,
        current: true,
    })
}

pub(super) fn apply(repository: &git2::Repository, tree: &git2::Tree<'_>) -> Result<(), Fault> {
    // libgit2 publishes the index after updating files. Reject an already-held
    // lock before entering checkout; a later external race remains uncertain.
    let location = index::location(repository)?;
    let metadata = path::root(location.parent().unwrap())?;
    match metadata.symlink_metadata("index.lock") {
        Ok(_) => {
            return Err(Fault::new(
                ErrorCode::Busy,
                "another Git index writer holds the lock",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(error)),
    }
    let mut checkout = CheckoutBuilder::new();
    checkout.safe().update_index(true).overwrite_ignored(false);
    repository
        .checkout_tree(tree.as_object(), Some(&mut checkout))
        .map_err(|error| {
            // With ALLOW_CONFLICTS off, libgit2 rejects conflicts while planning
            // actions, before file updates. Other failures may be partial.
            if error.code() == git2::ErrorCode::Conflict {
                Fault::new(
                    ErrorCode::Conflict,
                    "working changes would be overwritten by checkout",
                )
            } else {
                unknown()
            }
        })
}

fn unknown() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "checkout may have changed files; inspect the worktree before retrying",
    )
}
