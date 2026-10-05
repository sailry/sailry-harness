//! Adapted from sailry-code 67ae9fa0 sailry-git service.rs delete_branch (Apache-2.0).
use super::super::{check_root, fault, open, path, revision, worktrees};
use git2::{Branch, BranchType, Config, Oid, Reference};
use sailry_protocol::{ErrorCode, Fault};
use std::path::Path;

pub(crate) fn remove(
    root: &Path,
    name: &str,
    commit: &str,
    expected_head: &str,
    expected_branch: Option<&str>,
) -> Result<(), Fault> {
    if name.len() > 1024 || !Branch::name_is_valid(name).map_err(fault)? {
        return Err(Fault::new(ErrorCode::InvalidRequest, "invalid branch name"));
    }
    let tip = full_oid(commit)?;
    let head = full_oid(expected_head)?;
    let source = expected_branch.map(|name| format!("refs/heads/{name}"));
    if source
        .as_ref()
        .is_some_and(|name| !Reference::is_valid_name(name))
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid current branch name",
        ));
    }
    let retained = path::root(root)?;
    let repository = open(root)?;
    // Freeze the displayed checkout while checking reachability. The native
    // branch operation compares the target ref's old OID under its own lock.
    let mut locks = repository.transaction().map_err(fault)?;
    locks.lock_ref("HEAD").map_err(fault)?;
    if let Some(source) = &source {
        locks.lock_ref(source).map_err(fault)?;
    }
    let current = repository.find_reference("HEAD").map_err(fault)?;
    if current.symbolic_target().map_err(fault)? != source.as_deref()
        || current.resolve().map_err(fault)?.target() != Some(head)
    {
        return Err(revision::conflict());
    }
    let mut branch = repository
        .find_branch(name, BranchType::Local)
        .map_err(fault)?;
    if branch.get().target() != Some(tip) {
        return Err(revision::conflict());
    }
    let target = format!("refs/heads/{name}");
    if source.as_deref() == Some(&target) {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "cannot delete the current branch",
        ));
    }
    worktrees::check_available(&repository, root, &target)?;
    if head != tip && !repository.graph_descendant_of(head, tip).map_err(fault)? {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "branch has unmerged commits",
        ));
    }
    check_root(&retained, root)?;
    // This operation needs the repository's local tracking configuration. Unlike
    // checkout, deleting a ref does not invoke filters, hooks or transport helpers.
    // Do not make user/global configuration a writable branch metadata source.
    repository
        .set_config(&Config::open(&repository.commondir().join("config")).map_err(fault)?)
        .map_err(fault)?;
    // libgit2 removes tracking configuration before publishing ref deletion.
    // A failure here may already have changed metadata, so never replay it.
    branch.delete().map_err(|_| unknown())?;
    check_root(&retained, root).map_err(|_| unknown())?;
    Ok(())
}

fn full_oid(value: &str) -> Result<Oid, Fault> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "expected a full commit identifier",
        ));
    }
    Oid::from_str(value).map_err(fault)
}

fn unknown() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "branch deletion may have changed metadata; inspect the repository before retrying",
    )
}
