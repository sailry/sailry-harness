//! Adapted from sailry-code 67ae9fa0 service.rs merge_branch_into_worktree (Apache-2.0).
//! Native in-memory merging leaves conflicts untouched; checkout never uses force.
use super::{check_root, checkout, commit, fault, open, path, revision};
use git2::{Branch, BranchType, Oid, RepositoryState, StatusOptions};
use sailry_protocol::{ErrorCode, Fault, GitMerge, GitMergeKind};
use std::path::Path;

pub(crate) fn branch(
    root: &Path,
    name: &str,
    commit: &str,
    expected_head: &str,
    expected_branch: &str,
    expected_index: &str,
) -> Result<GitMerge, Fault> {
    for name in [name, expected_branch] {
        if name.len() > 1024 || !Branch::name_is_valid(name).map_err(fault)? {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "invalid merge branch name",
            ));
        }
    }
    if commit.len() != 40 || expected_head.len() != 40 {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "expected full merge commit identifiers",
        ));
    }
    let source_id = Oid::from_str(commit).map_err(fault)?;
    let target_id = Oid::from_str(expected_head).map_err(fault)?;
    let retained = path::root(root)?;
    let repository = open(root)?;
    if repository.state() != RepositoryState::Clean {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "finish the current Git operation before merging",
        ));
    }
    let source_ref = format!("refs/heads/{name}");
    let target_ref = format!("refs/heads/{expected_branch}");
    let mut refs = repository.transaction().map_err(fault)?;
    refs.lock_ref("HEAD").map_err(fault)?;
    refs.lock_ref(&target_ref).map_err(fault)?;
    if source_ref != target_ref {
        refs.lock_ref(&source_ref).map_err(fault)?;
    }
    revision::check(&repository, &target_ref, Some(target_id), expected_index)?;
    let source = repository
        .find_branch(name, BranchType::Local)
        .map_err(fault)?;
    if source.get().target() != Some(source_id) {
        return Err(revision::conflict());
    }
    if source_id == target_id
        || repository
            .graph_descendant_of(target_id, source_id)
            .map_err(fault)?
    {
        return Ok(GitMerge {
            kind: GitMergeKind::UpToDate,
            commit: target_id.to_string(),
        });
    }
    let mut status = StatusOptions::new();
    status.include_untracked(true).recurse_untracked_dirs(true);
    if !repository
        .statuses(Some(&mut status))
        .map_err(fault)?
        .is_empty()
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "commit or stash target worktree changes before merging",
        ));
    }
    let source = repository.find_commit(source_id).map_err(fault)?;
    let target = repository.find_commit(target_id).map_err(fault)?;
    let (id, kind) = if repository
        .graph_descendant_of(source_id, target_id)
        .map_err(fault)?
    {
        (source_id, GitMergeKind::FastForward)
    } else {
        // merge_commits handles recursive merge bases without publishing an index or MERGE_HEAD.
        let mut merged = repository
            .merge_commits(&target, &source, None)
            .map_err(fault)?;
        if merged.has_conflicts() {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "branches have merge conflicts; target worktree is unchanged",
            ));
        }
        let signature = commit::signature(&repository)?;
        let tree = merged.write_tree_to(&repository).map_err(fault)?;
        let tree = repository.find_tree(tree).map_err(fault)?;
        let id = repository
            .commit(
                None,
                &signature,
                &signature,
                &format!("Merge branch '{name}' into {expected_branch}"),
                &tree,
                &[&target, &source],
            )
            .map_err(fault)?;
        (id, GitMergeKind::MergeCommit)
    };
    let tree = repository
        .find_commit(id)
        .map_err(fault)?
        .tree()
        .map_err(fault)?;
    check_root(&retained, root)?;
    revision::check(&repository, &target_ref, Some(target_id), expected_index)?;
    refs.set_target(&target_ref, id, None, &format!("merge {name}"))
        .map_err(fault)?;
    checkout::apply(&repository, &tree)?;
    check_root(&retained, root).map_err(|_| unknown())?;
    refs.commit().map_err(|_| unknown())?;
    if repository.head().map_err(|_| unknown())?.target() != Some(id) {
        return Err(unknown());
    }
    Ok(GitMerge {
        kind,
        commit: id.to_string(),
    })
}

fn unknown() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "merge may have changed files or references; inspect the worktree before retrying",
    )
}
