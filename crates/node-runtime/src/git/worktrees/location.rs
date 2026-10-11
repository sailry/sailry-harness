//! Repository-local metadata stays outside user ignore files and file tools.
use super::super::{io_error, open};
use cap_fs_ext::{DirExt, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions;
use sailry_protocol::{ErrorCode, Fault};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(crate) fn capture(root: &Path) -> Result<Option<PathBuf>, Fault> {
    if open(root).is_ok() {
        return prepare(root).map(Some);
    }
    // Commands also support roots whose Git markers cannot be used by managed
    // creation. Preserve that access while retaining any existing child boundary.
    let anchor = crate::files::path::root(root)?;
    match anchor.symlink_metadata(".sailry") {
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {}
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    }
    let directory = anchor.open_dir_nofollow(".sailry").map_err(io_error)?;
    match directory.symlink_metadata("worktrees") {
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {
            Ok(Some(crate::workspaces::worktrees(root)))
        }
        Ok(_) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
    }
}

pub(crate) fn prepare(root: &Path) -> Result<PathBuf, Fault> {
    let repository = open(root)?;
    let metadata =
        crate::files::path::root(&repository.commondir().canonicalize().map_err(io_error)?)?;
    directory(&metadata, "info")?;
    let info = metadata.open_dir_nofollow("info").map_err(io_error)?;
    match info.symlink_metadata("exclude") {
        Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => {}
        Ok(_) => return Err(Fault::invalid("Git exclusion entry must be a regular file")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(error)),
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .append(true)
        .create(true)
        .follow(cap_fs_ext::FollowSymlinks::No);
    let mut exclude = info.open_with("exclude", &options).map_err(io_error)?;
    if !exclude.metadata().map_err(io_error)?.is_file() {
        return Err(Fault::invalid("Git exclusion entry must be a regular file"));
    }
    let mut bytes = Vec::new();
    (&mut exclude)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > 1024 * 1024 {
        return Err(Fault::invalid("Git exclusion entry exceeds the size limit"));
    }
    if !bytes
        .split(|byte| *byte == b'\n')
        .any(|line| line == b".sailry/" || line == b".sailry/\r")
    {
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            exclude.write_all(b"\n").map_err(io_error)?;
        }
        exclude.write_all(b".sailry/\n").map_err(io_error)?;
        exclude.sync_all().map_err(io_error)?;
    }
    let anchor = crate::files::path::root(root)?;
    directory(&anchor, ".sailry")?;
    let metadata = anchor.open_dir_nofollow(".sailry").map_err(io_error)?;
    directory(&metadata, "worktrees")?;
    metadata.open_dir_nofollow("worktrees").map_err(io_error)?;
    Ok(crate::workspaces::worktrees(root))
}

fn directory(root: &cap_std::fs::Dir, name: &str) -> Result<(), Fault> {
    match root.create_dir(name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = root.symlink_metadata(name).map_err(io_error)?;
            if metadata.is_dir() && !metadata.is_symlink() {
                Ok(())
            } else {
                Err(Fault::new(
                    ErrorCode::Conflict,
                    "repository metadata directory is unavailable",
                ))
            }
        }
        Err(error) => Err(io_error(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_ignore_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let repository = git2::Repository::init(&root).unwrap();
        let exclude = repository.commondir().join("info/exclude");
        std::fs::write(&exclude, "# Preserve this\nprivate/").unwrap();
        std::fs::write(root.join(".gitignore"), "user-owned\n").unwrap();
        let first = prepare(&root).unwrap();
        assert_eq!(first, crate::workspaces::worktrees(&root));
        assert!(first.is_dir());
        assert_eq!(
            std::fs::read_to_string(&exclude).unwrap(),
            "# Preserve this\nprivate/\n.sailry/\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".gitignore")).unwrap(),
            "user-owned\n"
        );
        prepare(&root).unwrap();
        assert_eq!(
            std::fs::read_to_string(exclude).unwrap(),
            "# Preserve this\nprivate/\n.sailry/\n"
        );
        assert!(
            repository
                .status_should_ignore(Path::new(".sailry/worktrees/file"))
                .unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_metadata_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap().join("project");
        let outside = temp.path().canonicalize().unwrap().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        git2::Repository::init(&root).unwrap();
        std::os::unix::fs::symlink(&outside, root.join(".sailry")).unwrap();
        assert_eq!(prepare(&root).unwrap_err().code, ErrorCode::Conflict);
        assert!(!outside.join("worktrees").exists());
    }

    #[test]
    fn retains_existing_scope() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        assert!(capture(&root).unwrap().is_none());
        let directory = crate::workspaces::worktrees(&root);
        std::fs::create_dir_all(&directory).unwrap();
        assert_eq!(capture(&root).unwrap(), Some(directory));
    }
}
