use super::{git_error, index, open};
use git2::{Oid, Repository};
use sailry_protocol::{ErrorCode, Fault};

/// Resolution follows sailry-code 67ae9fa0 command_support.rs branch_commit (Apache-2.0).
pub(super) fn resolve(root: &std::path::Path, revision: &str) -> Result<String, Fault> {
    if revision.is_empty() || revision.len() > 1024 || revision.chars().any(char::is_control) {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid Git revision",
        ));
    }
    let repository = open(root)?;
    let commit = repository
        .revparse_single(revision)
        .map_err(git_error)?
        .peel_to_commit()
        .map_err(git_error)?;
    Ok(commit.id().to_string())
}

pub(super) fn check(
    repository: &Repository,
    reference: &str,
    expected_head: Option<Oid>,
    expected_index: &str,
) -> Result<(), Fault> {
    let head = repository.find_reference("HEAD").map_err(git_error)?;
    if head.symbolic_target().map_err(git_error)?.unwrap_or("HEAD") != reference {
        return Err(conflict());
    }
    let target = match head.resolve() {
        Ok(head) => head.target(),
        Err(error) if error.code() == git2::ErrorCode::NotFound && expected_head.is_none() => None,
        Err(error) => return Err(git_error(error)),
    };
    if target != expected_head || index::revision(repository)? != expected_index {
        return Err(conflict());
    }
    Ok(())
}

pub(super) fn conflict() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "Git branch, HEAD, or index changed; review before retrying",
    )
}
