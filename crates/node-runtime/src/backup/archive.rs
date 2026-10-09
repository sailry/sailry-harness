//! Profile snapshots retain capabilities and publish no symlinks or special files.
use super::{MAX_BYTES, MAX_FILES, MAX_MANIFEST, cancelled, io};
use crate::files;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, NodeId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MANIFEST: &str = "sailry-backup.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct File {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub mode: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub version: u32,
    pub node: NodeId,
    pub original_profile: PathBuf,
    pub files: Vec<File>,
}

pub(crate) struct Snapshot {
    pub directory: tempfile::TempDir,
    pub manifest: Manifest,
    size: u64,
    paths: BTreeSet<String>,
}

impl Snapshot {
    pub(crate) fn new(profile: &Path, node: NodeId) -> Result<Self, Fault> {
        let temporary = crate::profile::temporary(profile)?;
        let directory = tempfile::Builder::new()
            .prefix("backup-")
            .tempdir_in(temporary)
            .map_err(io)?;
        Ok(Self {
            directory,
            manifest: Manifest {
                version: 1,
                node,
                original_profile: profile.to_owned(),
                files: vec![],
            },
            size: 0,
            paths: BTreeSet::new(),
        })
    }

    pub(crate) fn database_path(&self) -> Result<PathBuf, Fault> {
        let directory = files::path::root(self.directory.path())?;
        directory.create_dir("storage").map_err(io)?;
        let storage = directory.open_dir_nofollow("storage").map_err(io)?;
        let _ = create(&storage, "node.sqlite3")?;
        Ok(self.directory.path().join("storage/node.sqlite3"))
    }

    pub(crate) fn database(&mut self, cancel: &CancellationToken) -> Result<(), Fault> {
        let directory = files::path::root(&self.directory.path().join("storage"))?;
        let mut file = files::open_regular(&directory, "node.sqlite3")?.into_std();
        let size = file.metadata().map_err(io)?.len();
        let sha256 = hash(&mut file, size, cancel)?;
        self.add(File {
            path: "storage/node.sqlite3".into(),
            size,
            sha256,
            mode: 0o600,
        })
    }

    pub(crate) fn capture(
        &mut self,
        roots: &[String],
        cancel: &CancellationToken,
    ) -> Result<(), Fault> {
        let root = files::path::root(&self.manifest.original_profile)?;
        for path in roots {
            let managed = crate::workspaces::managed(
                &self.manifest.original_profile.join(path),
                &self.manifest.original_profile,
            );
            let parts = files::path::components(path, false)?;
            let (name, parent_names) = parts
                .split_last()
                .ok_or_else(|| Fault::invalid("backup source path is empty"))?;
            let parent = match files::path::descend(root.try_clone().map_err(io)?, parent_names) {
                Ok(parent) => parent,
                Err(error) if managed && error.code == ErrorCode::NotFound => continue,
                Err(error) => return Err(error),
            };
            match parent.symlink_metadata(name) {
                Err(error) if managed && error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(io(error)),
                Ok(_) => (),
            }
            self.copy(&parent, name, path, 0, cancel)?;
            if managed
                && let Some(metadata) = super::git::prepare(
                    &self.manifest.original_profile.join(path),
                    self.directory.path(),
                    cancel,
                )?
            {
                let pointer = format!("{path}/.git");
                let (parent, name) = parents(self.directory.path(), &pointer)?;
                parent.remove_file(&name).map_err(io)?;
                if let Some(index) = self
                    .manifest
                    .files
                    .iter()
                    .position(|file| file.path == pointer)
                {
                    self.size -= self.manifest.files.remove(index).size;
                    self.paths.remove(&pointer);
                } else {
                    return Err(Fault::invalid(
                        "linked workspace Git pointer was not captured",
                    ));
                }
                let directory = files::path::root(metadata.path())?;
                for entry in directory.entries().map_err(io)? {
                    let name = entry
                        .map_err(io)?
                        .file_name()
                        .into_string()
                        .map_err(|_| Fault::invalid("Git metadata names must be UTF-8"))?;
                    files::path::components(&name, false)?;
                    self.copy(&directory, &name, &format!("{pointer}/{name}"), 0, cancel)?;
                }
            }
        }
        Ok(())
    }

    fn copy(
        &mut self,
        source: &Dir,
        name: &str,
        relative: &str,
        depth: usize,
        cancel: &CancellationToken,
    ) -> Result<(), Fault> {
        cancelled(cancel)?;
        if depth > 64 {
            return Err(Fault::invalid("backup source nesting exceeds the limit"));
        }
        let metadata = source.symlink_metadata(name).map_err(io)?;
        if metadata.is_symlink() {
            return Err(Fault::invalid(format!(
                "backup source is a symbolic link: {relative}"
            )));
        }
        if metadata.is_dir() {
            let directory = source.open_dir_nofollow(name).map_err(io)?;
            for entry in directory.entries().map_err(io)? {
                let name = entry
                    .map_err(io)?
                    .file_name()
                    .into_string()
                    .map_err(|_| Fault::invalid("backup names must be UTF-8"))?;
                files::path::components(&name, false)?;
                self.copy(
                    &directory,
                    &name,
                    &format!("{relative}/{name}"),
                    depth + 1,
                    cancel,
                )?;
            }
            return Ok(());
        }
        if !metadata.is_file() {
            return Err(Fault::invalid(format!(
                "backup source is not a regular file: {relative}"
            )));
        }
        if self.paths.contains(relative) {
            return Ok(());
        }
        let mut input = files::open_regular(source, name)?.into_std();
        let before = input.metadata().map_err(io)?;
        if self.size.saturating_add(before.len()) > MAX_BYTES || self.paths.len() >= MAX_FILES {
            return Err(Fault::invalid("backup source exceeds the export limit"));
        }
        let (parent, name) = parents(self.directory.path(), relative)?;
        let mut output = create(&parent, &name)?.into_std();
        let (size, digest) = copy_hash(&mut input, &mut output, before.len(), cancel)?;
        output.sync_all().map_err(io)?;
        let after = input.metadata().map_err(io)?;
        if before.len() != after.len()
            || before.modified().map_err(io)? != after.modified().map_err(io)?
        {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                format!("backup source changed while reading: {relative}"),
            ));
        }
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            0o600 | (before.permissions().mode() & 0o100)
        };
        #[cfg(not(unix))]
        let mode = 0o600;
        self.add(File {
            path: relative.into(),
            size,
            sha256: digest,
            mode,
        })
    }

    fn add(&mut self, file: File) -> Result<(), Fault> {
        self.size = self
            .size
            .checked_add(file.size)
            .filter(|size| *size <= MAX_BYTES)
            .ok_or_else(|| Fault::invalid("backup source exceeds the export limit"))?;
        if !self.paths.insert(file.path.clone()) || self.paths.len() > MAX_FILES {
            return Err(Fault::invalid("duplicate or excessive backup files"));
        }
        self.manifest.files.push(file);
        Ok(())
    }

    pub(crate) fn pack(mut self, cancel: &CancellationToken) -> Result<Packed, Fault> {
        self.manifest
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        self.manifest.validate()?;
        let manifest = serde_json::to_vec(&self.manifest).map_err(io)?;
        if manifest.len() > MAX_MANIFEST {
            return Err(Fault::invalid("backup manifest exceeds the limit"));
        }
        let file = tempfile::NamedTempFile::new_in(self.directory.path()).map_err(io)?;
        let mut tar = tar::Builder::new(GzEncoder::new(
            file.reopen().map_err(io)?,
            Compression::fast(),
        ));
        append(
            &mut tar,
            MANIFEST,
            manifest.len() as u64,
            0o600,
            manifest.as_slice(),
        )?;
        let root = files::path::root(self.directory.path())?;
        for entry in &self.manifest.files {
            cancelled(cancel)?;
            let parts = files::path::components(&entry.path, false)?;
            let (name, parents) = parts.split_last().unwrap();
            let dir = files::path::descend(root.try_clone().map_err(io)?, parents)?;
            let input = files::open_regular(&dir, name)?.into_std();
            let result = append(
                &mut tar,
                &entry.path,
                entry.size,
                entry.mode,
                Controlled { input, cancel },
            );
            cancelled(cancel)?;
            result?;
        }
        let output = tar.into_inner().map_err(io)?.finish().map_err(io)?;
        output.sync_all().map_err(io)?;
        let size = output.metadata().map_err(io)?.len();
        let sha256 = hash(&mut file.reopen().map_err(io)?, size, cancel)?;
        Ok(Packed {
            snapshot: self,
            file,
            size,
            sha256,
        })
    }
}

pub(crate) struct Packed {
    pub snapshot: Snapshot,
    pub file: tempfile::NamedTempFile,
    pub size: u64,
    pub sha256: String,
}

impl Manifest {
    pub(crate) fn validate(&self) -> Result<(), Fault> {
        if self.version != 1 || !self.original_profile.is_absolute() || self.files.len() > MAX_FILES
        {
            return Err(Fault::invalid("invalid backup manifest"));
        }
        let mut total = 0u64;
        let mut paths = BTreeSet::new();
        for file in &self.files {
            files::path::components(&file.path, false)?;
            let valid = file.path == "link.key"
                || file.path == "storage/node.sqlite3"
                || file.path.starts_with("plugins/packages/")
                || file.path.starts_with("attachments/")
                || Path::new(&file.path).ancestors().skip(1).any(|parent| {
                    crate::workspaces::managed(
                        &self.original_profile.join(parent),
                        &self.original_profile,
                    )
                });
            if !valid
                || !paths.insert(file.path.as_str())
                || !digest(&file.sha256)
                || file.mode & !0o700 != 0
            {
                return Err(Fault::invalid("invalid backup file metadata"));
            }
            total = total
                .checked_add(file.size)
                .filter(|size| *size <= MAX_BYTES)
                .ok_or_else(|| Fault::invalid("backup content exceeds the restore limit"))?;
        }
        if !paths.contains("storage/node.sqlite3") || !paths.contains("link.key") {
            return Err(Fault::invalid("backup database or identity is missing"));
        }
        Ok(())
    }
}

pub(crate) fn unpack(
    input: std::fs::File,
    destination: &Path,
    cancel: &CancellationToken,
) -> Result<Manifest, Fault> {
    let mut archive = tar::Archive::new(GzDecoder::new(input));
    let mut entries = archive.entries().map_err(io)?;
    let mut first = entries
        .next()
        .ok_or_else(|| Fault::invalid("backup manifest is missing"))?
        .map_err(io)?;
    if !first.header().entry_type().is_file()
        || first.path().map_err(io)? != Path::new(MANIFEST)
        || first.size() > MAX_MANIFEST as u64
    {
        return Err(Fault::invalid(
            "backup manifest must be the first regular file",
        ));
    }
    let mut bytes = Vec::new();
    first.read_to_end(&mut bytes).map_err(io)?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(io)?;
    manifest.validate()?;
    let mut expected: std::collections::BTreeMap<_, _> = manifest
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    for entry in entries {
        cancelled(cancel)?;
        let mut entry = entry.map_err(io)?;
        if !entry.header().entry_type().is_file() {
            return Err(Fault::invalid("backup entries must be regular files"));
        }
        let path = entry.path().map_err(io)?.into_owned();
        let relative = path
            .to_str()
            .ok_or_else(|| Fault::invalid("backup names must be UTF-8"))?;
        files::path::components(relative, false)?;
        let spec = expected
            .remove(relative)
            .ok_or_else(|| Fault::invalid("backup file is unexpected or duplicated"))?;
        if entry.size() != spec.size {
            return Err(Fault::invalid("backup file size differs from its manifest"));
        }
        let (parent, name) = parents(destination, relative)?;
        let mut file = create(&parent, &name)?.into_std();
        let (_, sha256) = copy_hash(&mut entry, &mut file, spec.size, cancel)?;
        if sha256 != spec.sha256 {
            return Err(Fault::invalid(
                "backup file checksum differs from its manifest",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(spec.mode))
                .map_err(io)?;
        }
        file.sync_all().map_err(io)?;
    }
    if !expected.is_empty() {
        return Err(Fault::invalid("backup contains missing files"));
    }
    Ok(manifest)
}

fn append<W: Write>(
    tar: &mut tar::Builder<W>,
    path: &str,
    size: u64,
    mode: u32,
    input: impl Read,
) -> Result<(), Fault> {
    let mut header = tar::Header::new_gnu();
    header.set_size(size);
    header.set_mode(mode);
    header.set_cksum();
    tar.append_data(&mut header, path, input).map_err(io)
}

pub(crate) fn parents(root: &Path, relative: &str) -> Result<(Dir, String), Fault> {
    parents_from(files::path::root(root)?, relative)
}

fn parents_from(mut directory: Dir, relative: &str) -> Result<(Dir, String), Fault> {
    let parts = files::path::components(relative, false)?;
    let (name, parents) = parts.split_last().unwrap();
    for part in parents {
        match directory.create_dir(part) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io(error)),
        }
        directory = directory.open_dir_nofollow(part).map_err(io)?;
        #[cfg(unix)]
        {
            use cap_std::fs::PermissionsExt;
            directory
                .set_permissions(".", cap_std::fs::Permissions::from_mode(0o700))
                .map_err(io)?;
        }
    }
    Ok((directory, (*name).into()))
}

fn create(directory: &Dir, name: &str) -> Result<cap_std::fs::File, Fault> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    cap_std::fs::OpenOptionsExt::mode(&mut options, 0o600);
    directory.open_with(name, &options).map_err(io)
}

pub(crate) fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|ch| ch.is_ascii_hexdigit())
}

pub(crate) fn hash(
    input: &mut impl Read,
    size: u64,
    cancel: &CancellationToken,
) -> Result<String, Fault> {
    copy_hash(input, &mut std::io::sink(), size, cancel).map(|(_, digest)| digest)
}

fn copy_hash(
    input: &mut impl Read,
    output: &mut impl Write,
    expected: u64,
    cancel: &CancellationToken,
) -> Result<(u64, String), Fault> {
    let mut digest = Sha256::new();
    let mut size = 0u64;
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        cancelled(cancel)?;
        let length = input.read(&mut buffer).map_err(io)?;
        if length == 0 {
            break;
        }
        size = size
            .checked_add(length as u64)
            .filter(|size| *size <= expected)
            .ok_or_else(|| Fault::invalid("backup file exceeds its declared size"))?;
        digest.update(&buffer[..length]);
        output.write_all(&buffer[..length]).map_err(io)?;
    }
    if size != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "backup file size changed",
        ));
    }
    Ok((size, format!("{:x}", digest.finalize())))
}

struct Controlled<'a, R> {
    input: R,
    cancel: &'a CancellationToken,
}
impl<R: Read> Read for Controlled<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(std::io::Error::other("backup packing cancelled"));
        }
        self.input.read(buffer)
    }
}

pub(crate) fn transfer(
    source: &Path,
    destination: &Dir,
    manifest: &Manifest,
    cancel: &CancellationToken,
) -> Result<(), Fault> {
    let source = files::path::root(source)?;
    for spec in &manifest.files {
        cancelled(cancel)?;
        let parts = files::path::components(&spec.path, false)?;
        let (name, parents) = parts.split_last().unwrap();
        let parent = files::path::descend(source.try_clone().map_err(io)?, parents)?;
        let mut input = files::open_regular(&parent, name)?.into_std();
        let (parent, name) = parents_from(destination.try_clone().map_err(io)?, &spec.path)?;
        let mut output = create(&parent, &name)?.into_std();
        let (_, digest) = copy_hash(&mut input, &mut output, spec.size, cancel)?;
        if digest != spec.sha256 {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "verified restore staging changed",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            output
                .set_permissions(std::fs::Permissions::from_mode(spec.mode))
                .map_err(io)?;
        }
        output.sync_all().map_err(io)?;
    }
    Ok(())
}
