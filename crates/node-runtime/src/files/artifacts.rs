//! Immutable artifact revisions use confined, no-follow profile capabilities.
use super::{io_error, path, save};
use cap_fs_ext::DirExt;
use sailry_protocol::{ArtifactId, ErrorCode, Fault, SessionId, artifact::MAX_HTML_BYTES};
use std::{io::Read, path::Path};

/// The caller confirms this revision has no committed metadata before publication.
pub(crate) fn write(
    profile: &Path,
    session: SessionId,
    id: ArtifactId,
    revision: u32,
    html: &str,
) -> Result<String, Fault> {
    let mut directory = path::root(profile)?;
    for name in ["artifacts".to_owned(), session.to_string(), id.to_string()] {
        match directory.create_dir(&name) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use cap_std::fs::PermissionsExt;
                    directory
                        .set_permissions(&name, cap_std::fs::Permissions::from_mode(0o700))
                        .map_err(io_error)?;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_error(error)),
        }
        directory = directory.open_dir_nofollow(&name).map_err(io_error)?;
    }
    let root = profile
        .join("artifacts")
        .join(session.to_string())
        .join(id.to_string());
    let digest = blake3::hash(html.as_bytes()).to_hex().to_string();
    let name = format!("{revision}.html");
    let expected = match directory.symlink_metadata(&name) {
        Ok(_) => {
            let file = super::open_regular(&directory, &name)?;
            let mut bytes = Vec::new();
            file.take(MAX_HTML_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(io_error)?;
            if bytes.len() > MAX_HTML_BYTES {
                return Err(Fault::invalid("artifact orphan exceeds its size limit"));
            }
            Some(blake3::hash(&bytes).to_hex().to_string())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(io_error(error)),
    };
    save::Target::prepare(
        &root,
        &name,
        expected.as_deref(),
        MAX_HTML_BYTES as u64,
        None,
    )?
    .write(
        html.as_bytes(),
        html.len() as u64,
        &digest,
        #[cfg(test)]
        &|_| {},
    )?;
    Ok(digest)
}

pub(crate) fn read(
    profile: &Path,
    session: SessionId,
    id: ArtifactId,
    revision: u32,
    size: u64,
    digest: &str,
) -> Result<String, Fault> {
    let parts = ["artifacts".to_owned(), session.to_string(), id.to_string()];
    let references: Vec<_> = parts.iter().map(String::as_str).collect();
    let directory = path::descend(path::root(profile)?, &references)?;
    let file = super::open_regular(&directory, &format!("{revision}.html"))?;
    let mut bytes = Vec::new();
    file.take(MAX_HTML_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 != size || blake3::hash(&bytes).to_hex().as_str() != digest {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "artifact content changed",
        ));
    }
    String::from_utf8(bytes).map_err(|_| Fault::invalid("artifact content is not UTF-8"))
}

pub(crate) fn remove(profile: &Path, session: SessionId) -> Result<(), Fault> {
    let directory = match path::root(profile)?.open_dir_nofollow("artifacts") {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(error)),
    };
    match directory.remove_dir_all(session.to_string()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_only_unreferenced_owner() {
        let profile = tempfile::tempdir().unwrap();
        let root = profile.path().canonicalize().unwrap();
        let removed = SessionId::new();
        let retained = SessionId::new();
        let id = ArtifactId::new();
        write(&root, removed, id, 1, "removed").unwrap();
        let digest = write(&root, retained, id, 1, "retained").unwrap();
        let workspace = crate::workspaces::session(&root, removed);
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("output.txt"), "durable output").unwrap();
        remove(&root, removed).unwrap();
        remove(&root, removed).unwrap();
        assert!(!root.join("artifacts").join(removed.to_string()).exists());
        assert_eq!(
            read(&root, retained, id, 1, 8, &digest).unwrap(),
            "retained"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.join("output.txt")).unwrap(),
            "durable output"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_linked_cleanup() {
        let profile = tempfile::tempdir().unwrap();
        let root = profile.path().canonicalize().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let session = SessionId::new();
        let directory = outside.path().join(session.to_string());
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("keep.html"), "retained").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("artifacts")).unwrap();
        assert!(remove(&root, session).is_err());
        assert_eq!(
            std::fs::read_to_string(directory.join("keep.html")).unwrap(),
            "retained"
        );
    }

    #[test]
    fn replaces_unreferenced_publication() {
        let profile = tempfile::tempdir().unwrap();
        let root = profile.path().canonicalize().unwrap();
        let session = SessionId::new();
        let id = ArtifactId::new();
        write(&root, session, id, 2, "unfinished").unwrap();
        let html = "<h1>Recovered revision</h1>";
        let digest = write(&root, session, id, 2, html).unwrap();
        assert_eq!(
            read(&root, session, id, 2, html.len() as u64, &digest).unwrap(),
            html
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_linked_publication() {
        let profile = tempfile::tempdir().unwrap();
        let root = profile.path().canonicalize().unwrap();
        let session = SessionId::new();
        let id = ArtifactId::new();
        write(&root, session, id, 1, "original").unwrap();
        let target = root.join("outside.html");
        std::fs::write(&target, "untouched").unwrap();
        let next = root
            .join("artifacts")
            .join(session.to_string())
            .join(id.to_string())
            .join("2.html");
        std::os::unix::fs::symlink(&target, next).unwrap();
        assert!(write(&root, session, id, 2, "changed").is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "untouched");
    }
}
