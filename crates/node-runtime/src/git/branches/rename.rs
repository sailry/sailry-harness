//! Adapted from sailry-code 67ae9fa0 sailry-git service.rs rename_branch (Apache-2.0).
use super::super::{check_root, fault, open, path, revision, worktrees};
use git2::{Branch, BranchType, Config, Oid};
use sailry_protocol::{ErrorCode, Fault, GitBranch};
use std::path::Path;

pub(crate) fn rename(
    root: &Path,
    name: &str,
    new_name: &str,
    commit: &str,
) -> Result<GitBranch, Fault> {
    for name in [name, new_name] {
        if name.len() > 1024 || !Branch::name_is_valid(name).map_err(fault)? {
            return Err(Fault::new(ErrorCode::InvalidRequest, "invalid branch name"));
        }
    }
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "expected a full commit identifier",
        ));
    }
    let tip = Oid::from_str(commit).map_err(fault)?;
    let retained = path::root(root)?;
    let repository = open(root)?;
    let mut branch = repository
        .find_branch(name, BranchType::Local)
        .map_err(fault)?;
    if branch.get().target() != Some(tip) {
        return Err(revision::conflict());
    }
    if name == new_name {
        return Ok(GitBranch {
            name: name.into(),
            commit: tip.to_string(),
            remote: false,
            current: branch.is_head(),
        });
    }
    match repository.find_branch(new_name, BranchType::Local) {
        Ok(_) => return Err(Fault::new(ErrorCode::Conflict, "branch already exists")),
        Err(error) if error.code() == git2::ErrorCode::NotFound => {}
        Err(error) => return Err(fault(error)),
    }
    worktrees::check_mutable(&repository, &format!("refs/heads/{name}"))?;
    repository
        .set_config(&Config::open(&repository.commondir().join("config")).map_err(fault)?)
        .map_err(fault)?;
    check_root(&retained, root)?;
    // libgit2 moves the ref/reflog, updates all owning HEADs and then tracking
    // config. These steps are not atomic; do not force, roll back or replay them.
    let renamed = branch.rename(new_name, false).map_err(|_| unknown())?;
    if renamed.get().target() != Some(tip) {
        return Err(unknown());
    }
    check_root(&retained, root).map_err(|_| unknown())?;
    Ok(GitBranch {
        name: new_name.into(),
        commit: tip.to_string(),
        remote: false,
        current: renamed.is_head(),
    })
}

fn unknown() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "branch rename may have changed refs or configuration; inspect the repository before retrying",
    )
}
