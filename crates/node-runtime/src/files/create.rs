//! Entry creation follows harbor-file-service 727ce0a worktree.rs (Apache-2.0).
//! Reuse the shared capability traversal rather than ambient child paths.
use super::{io_error, path};
use sailry_protocol::{ErrorCode, Fault};
use std::path::Path;

pub(crate) fn directory(root: &Path, relative: &str) -> Result<(), Fault> {
    let parts = path::components(relative, false)?;
    let (name, parents) = parts.split_last().unwrap();
    let parent = path::descend(path::root(root)?, parents)?;
    parent.create_dir(name).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            Fault::new(ErrorCode::Conflict, "an entry already exists at this path")
        } else {
            io_error(error)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_existing_entries() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(root.join("file"), "keep").unwrap();
        directory(&root, "folder").unwrap();
        for path in ["file", "folder"] {
            assert_eq!(
                directory(&root, path).unwrap_err().code,
                ErrorCode::Conflict
            );
        }
        assert_eq!(std::fs::read_to_string(root.join("file")).unwrap(), "keep");
    }

    #[test]
    fn requires_existing_parent() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        assert_eq!(
            directory(&root, "missing/child").unwrap_err().code,
            ErrorCode::NotFound
        );
        assert!(!root.join("missing").exists());
        directory(&root, "资料").unwrap();
        directory(&root, "资料/新目录").unwrap();
        assert!(root.join("资料/新目录").is_dir());
        for path in ["", "../outside", "/absolute", "folder/../other"] {
            assert_eq!(
                directory(&root, path).unwrap_err().code,
                ErrorCode::InvalidRequest
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_linked_parents() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("linked")).unwrap();
        assert!(directory(&root, "linked/child").is_err());
        assert!(!outside.path().join("child").exists());
    }
}
