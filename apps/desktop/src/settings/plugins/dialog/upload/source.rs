use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, plugin::MAX_PACKAGE_BYTES};
use std::{
    fs::File,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
};

pub(super) struct Opened {
    pub file: File,
    pub directory: Option<PathBuf>,
}

pub(super) fn open(path: PathBuf, stop: CancellationToken) -> Result<Opened, Fault> {
    let path = path.canonicalize().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Fault::new(ErrorCode::NotFound, "plugin source no longer exists")
        } else {
            io_error(error)
        }
    })?;
    if path.is_dir() {
        let root = Dir::open_ambient_dir(&path, cap_std::ambient_authority()).map_err(io_error)?;
        let manifest = root.symlink_metadata("plugin.json").map_err(io_error)?;
        if !manifest.is_file() || manifest.is_symlink() {
            return Err(invalid());
        }
        let mut package = Package {
            zip: zip::ZipWriter::new(tempfile::tempfile().map_err(io_error)?),
            entries: 0,
            bytes: 0,
            stop,
        };
        package.append(&root, "", 0)?;
        let mut file = package.zip.finish().map_err(|_| invalid())?;
        file.rewind().map_err(io_error)?;
        Ok(Opened {
            file,
            directory: Some(path),
        })
    } else {
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        {
            return Err(invalid());
        }
        let parent = Dir::open_ambient_dir(
            path.parent().ok_or_else(invalid)?,
            cap_std::ambient_authority(),
        )
        .map_err(io_error)?;
        Ok(Opened {
            file: regular(&parent, Path::new(path.file_name().ok_or_else(invalid)?))?.into_std(),
            directory: None,
        })
    }
}

struct Package {
    zip: zip::ZipWriter<File>,
    entries: usize,
    bytes: u64,
    stop: CancellationToken,
}

impl Package {
    fn append(&mut self, dir: &Dir, prefix: &str, depth: usize) -> Result<(), Fault> {
        if depth > 32 {
            return Err(invalid());
        }
        for entry in dir.entries().map_err(io_error)? {
            if self.stop.is_cancelled() {
                return Err(Fault::new(
                    ErrorCode::Cancelled,
                    "plugin selection cancelled",
                ));
            }
            self.entries += 1;
            if self.entries > 1024 {
                return Err(invalid());
            }
            let name = entry
                .map_err(io_error)?
                .file_name()
                .into_string()
                .map_err(|_| invalid())?;
            if name.contains(['\\', ':']) || name.chars().any(char::is_control) {
                return Err(invalid());
            }
            let path = format!("{prefix}{name}");
            let metadata = dir.symlink_metadata(&name).map_err(io_error)?;
            if metadata.is_symlink() {
                return Err(invalid());
            }
            if metadata.is_dir() {
                self.zip
                    .add_directory(format!("{path}/"), zip::write::SimpleFileOptions::default())
                    .map_err(|_| invalid())?;
                self.append(
                    &dir.open_dir_nofollow(&name).map_err(io_error)?,
                    &format!("{path}/"),
                    depth + 1,
                )?;
            } else {
                let mut file = regular(dir, Path::new(&name))?;
                let before = file.metadata().map_err(io_error)?;
                self.bytes = self
                    .bytes
                    .checked_add(before.len())
                    .filter(|size| *size <= MAX_PACKAGE_BYTES)
                    .ok_or_else(invalid)?;
                let options = zip::write::SimpleFileOptions::default();
                #[cfg(unix)]
                let options = {
                    use cap_std::fs::PermissionsExt;
                    options.unix_permissions(before.permissions().mode() & 0o777)
                };
                self.zip.start_file(path, options).map_err(|_| invalid())?;
                let mut buffer = [0; 64 * 1024];
                let mut copied = 0;
                loop {
                    if self.stop.is_cancelled() {
                        return Err(Fault::new(
                            ErrorCode::Cancelled,
                            "plugin selection cancelled",
                        ));
                    }
                    let count = file.read(&mut buffer).map_err(io_error)?;
                    if count == 0 {
                        break;
                    }
                    copied += count as u64;
                    if copied > before.len() {
                        return Err(invalid());
                    }
                    self.zip.write_all(&buffer[..count]).map_err(io_error)?;
                }
                let after = file.metadata().map_err(io_error)?;
                if copied != before.len()
                    || after.len() != before.len()
                    || after.modified().ok() != before.modified().ok()
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}

fn regular(dir: &Dir, name: &Path) -> Result<cap_std::fs::File, Fault> {
    let metadata = dir.symlink_metadata(name).map_err(io_error)?;
    if !metadata.is_file() || metadata.is_symlink() {
        return Err(invalid());
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    cap_std::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    let file = dir.open_with(name, &options).map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(invalid());
    }
    Ok(file)
}

fn invalid() -> Fault {
    Fault::new(
        ErrorCode::InvalidRequest,
        "invalid plugin package or directory",
    )
}
fn io_error(error: std::io::Error) -> Fault {
    Fault::new(
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            ErrorCode::PermissionDenied
        } else {
            ErrorCode::InvalidRequest
        },
        "cannot read plugin package or directory",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_directory_contents() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("plugin.json"), b"{}").unwrap();
        std::fs::create_dir(root.path().join("skills")).unwrap();
        std::fs::write(root.path().join("skills/SKILL.md"), b"skill fixture").unwrap();
        let file = open(root.path().into(), CancellationToken::new()).unwrap();
        let mut zip = zip::ZipArchive::new(file.file).unwrap();
        assert_eq!(zip.len(), 3);
        let mut text = String::new();
        zip.by_name("skills/SKILL.md")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "skill fixture");
    }

    #[test]
    fn missing_manifest_and_cancellation() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            open(root.path().join("missing"), CancellationToken::new())
                .err()
                .unwrap()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            open(root.path().into(), CancellationToken::new())
                .err()
                .unwrap()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            io_error(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).code,
            ErrorCode::PermissionDenied
        );
        std::fs::write(root.path().join("plugin.json"), b"{}").unwrap();
        let stop = CancellationToken::new();
        stop.cancel();
        assert_eq!(
            open(root.path().into(), stop).err().unwrap().code,
            ErrorCode::Cancelled
        );
    }

    #[cfg(unix)]
    #[test]
    fn links_and_executables() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("plugin.json"), b"{}").unwrap();
        std::fs::write(root.path().join("tool"), b"executable fixture").unwrap();
        std::fs::set_permissions(
            root.path().join("tool"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(
            open(root.path().into(), CancellationToken::new())
                .unwrap()
                .file,
        )
        .unwrap();
        assert_ne!(zip.by_name("tool").unwrap().unix_mode().unwrap() & 0o111, 0);
        symlink("/private/secret", root.path().join("outside")).unwrap();
        assert!(open(root.path().into(), CancellationToken::new()).is_err());
    }
}
