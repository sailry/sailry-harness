//! Bounded package copying uses the same retained directory/file capabilities as project I/O.
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use sailry_protocol::{
    Fault,
    plugin::{Issue, IssueKind},
};
use std::io::{Read, Write};

use super::{invalid, io_error};

const MAX_ENTRIES: usize = 1024;
const MAX_BYTES: u64 = 128 * 1024 * 1024;
const MAX_DEPTH: usize = 32;

pub(super) fn names(directory: &Dir) -> Result<Vec<String>, Fault> {
    let mut names = Vec::new();
    for entry in directory.entries().map_err(io_error)? {
        if names.len() >= MAX_ENTRIES {
            return Err(invalid("plugin package has too many entries"));
        }
        let name = entry
            .map_err(io_error)?
            .file_name()
            .into_string()
            .map_err(|_| invalid("plugin package names must be valid UTF-8"))?;
        crate::files::path::components(&name, false)?;
        names.push(name);
    }
    names.sort();
    Ok(names)
}

pub(super) fn read(directory: &Dir, name: &str, limit: usize) -> Result<Option<Vec<u8>>, Fault> {
    match directory.symlink_metadata(name) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
        Ok(_) => {}
    }
    let file = crate::files::open_regular(directory, name)?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > limit {
        return Err(invalid("plugin file exceeds its size limit"));
    }
    Ok(Some(bytes))
}

pub(super) struct Tree {
    hash: blake3::Hasher,
    entries: usize,
    bytes: u64,
    pub issues: Vec<Issue>,
    pub files: std::collections::BTreeMap<String, blake3::Hash>,
    pub stop: sailry_link::CancellationToken,
}

impl Default for Tree {
    fn default() -> Self {
        Self {
            hash: blake3::Hasher::new_derive_key("Sailry plugin package v1"),
            entries: 0,
            bytes: 0,
            issues: Vec::new(),
            files: Default::default(),
            stop: sailry_link::CancellationToken::new(),
        }
    }
}

impl Tree {
    pub fn digest(&self) -> String {
        self.hash.finalize().to_hex().to_string()
    }

    pub fn walk(
        &mut self,
        source: &Dir,
        target: Option<&Dir>,
        prefix: &str,
        depth: usize,
    ) -> Result<(), Fault> {
        if depth > MAX_DEPTH {
            return Err(invalid("plugin package nesting is too deep"));
        }
        for name in names(source)? {
            self.check()?;
            self.entries += 1;
            if self.entries > MAX_ENTRIES {
                return Err(invalid("plugin package has too many entries"));
            }
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let metadata = source.symlink_metadata(&name).map_err(io_error)?;
            // An unavailable component does not invalidate unrelated components.
            // No-follow reads also reject a racing replacement after this check.
            if metadata.is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
                self.issues.push(Issue {
                    path,
                    kind: IssueKind::UnavailablePath,
                });
                continue;
            }
            self.hash.update(&(path.len() as u64).to_le_bytes());
            self.hash.update(path.as_bytes());
            if metadata.is_dir() {
                self.hash.update(b"directory");
                let source = source.open_dir_nofollow(&name).map_err(io_error)?;
                crate::files::path::identity::directory(&source)?;
                let target = target
                    .map(|target| {
                        target.create_dir(&name).map_err(io_error)?;
                        let directory = target.open_dir_nofollow(&name).map_err(io_error)?;
                        #[cfg(unix)]
                        {
                            use cap_std::fs::PermissionsExt;
                            directory
                                .set_permissions(".", cap_std::fs::Permissions::from_mode(0o700))
                                .map_err(io_error)?;
                        }
                        Ok::<_, Fault>(directory)
                    })
                    .transpose()?;
                self.walk(&source, target.as_ref(), &path, depth + 1)?;
                #[cfg(unix)]
                if let Some(target) = target {
                    target.into_std_file().sync_all().map_err(io_error)?;
                }
            } else {
                let mut file = crate::files::open_regular(source, &name)?;
                let before = file.metadata().map_err(io_error)?;
                let size = before.len();
                self.bytes = self
                    .bytes
                    .checked_add(size)
                    .filter(|size| *size <= MAX_BYTES)
                    .ok_or_else(|| invalid("plugin package exceeds the size limit"))?;
                let executable = executable(&before);
                self.hash.update(b"file");
                self.hash.update(&[executable as u8]);
                self.hash.update(&size.to_le_bytes());
                let mut output = target
                    .map(|target| {
                        target
                            .open_with(
                                &name,
                                cap_std::fs::OpenOptions::new().write(true).create_new(true),
                            )
                            .map_err(io_error)
                    })
                    .transpose()?;
                let mut copied = 0;
                let mut content = blake3::Hasher::new();
                let mut buffer = [0; 64 * 1024];
                loop {
                    self.check()?;
                    let count = file.read(&mut buffer).map_err(io_error)?;
                    if count == 0 {
                        break;
                    }
                    copied += count as u64;
                    if copied > size {
                        return Err(invalid("plugin file changed during installation"));
                    }
                    self.hash.update(&buffer[..count]);
                    content.update(&buffer[..count]);
                    if let Some(output) = &mut output {
                        output.write_all(&buffer[..count]).map_err(io_error)?;
                    }
                }
                let after = file.metadata().map_err(io_error)?;
                if copied != size
                    || after.len() != size
                    || after.modified().ok() != before.modified().ok()
                {
                    return Err(invalid("plugin file changed during installation"));
                }
                self.files.insert(path, content.finalize());
                if let Some(output) = output {
                    #[cfg(unix)]
                    {
                        use cap_std::fs::PermissionsExt;
                        output
                            .set_permissions(cap_std::fs::Permissions::from_mode(if executable {
                                0o700
                            } else {
                                0o600
                            }))
                            .map_err(io_error)?;
                    }
                    output.sync_all().map_err(io_error)?;
                }
            }
        }
        Ok(())
    }

    fn check(&self) -> Result<(), Fault> {
        if self.stop.is_cancelled() {
            return Err(Fault::new(
                sailry_protocol::ErrorCode::Cancelled,
                "plugin read was cancelled",
            ));
        }
        Ok(())
    }
}

fn executable(metadata: &cap_std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use cap_std::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}
