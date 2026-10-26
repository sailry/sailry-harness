use super::{fault, open};
use git2::Repository;
use sailry_protocol::{ErrorCode, Fault, GitHead, RepositoryKind};
use std::path::Path;

pub(super) fn read(root: &Path) -> Result<GitHead, Fault> {
    let repository = match open(root) {
        Ok(repository) => repository,
        Err(error) if error.code == ErrorCode::NotFound && !root.join(".git").exists() => {
            return Ok(GitHead {
                kind: RepositoryKind::Directory,
                branch: None,
                head: None,
            });
        }
        Err(error) => return Err(error),
    };
    project(&repository)
}

pub(super) fn project(repository: &Repository) -> Result<GitHead, Fault> {
    let head = match repository.head() {
        Ok(head) => Some(head),
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => None,
        Err(error) => return Err(fault(error)),
    };
    let branch = match &head {
        Some(head) if head.is_branch() => head.shorthand().ok().map(str::to_owned),
        Some(_) => None,
        None => repository
            .find_reference("HEAD")
            .map_err(fault)?
            .symbolic_target()
            .map_err(fault)?
            .and_then(|name| name.strip_prefix("refs/heads/"))
            .map(str::to_owned),
    };
    Ok(GitHead {
        kind: if head.is_some() {
            RepositoryKind::Ready
        } else {
            RepositoryKind::Unborn
        },
        branch,
        head: head.and_then(|head| head.target()).map(|id| id.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_directory_and_unborn_branch() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        assert_eq!(
            read(&root).unwrap(),
            GitHead {
                kind: RepositoryKind::Directory,
                branch: None,
                head: None,
            }
        );
        let repository = git2::Repository::init(&root).unwrap();
        repository.set_head("refs/heads/main").unwrap();
        assert_eq!(
            read(&root).unwrap(),
            GitHead {
                kind: RepositoryKind::Unborn,
                branch: Some("main".into()),
                head: None,
            }
        );
    }
}
