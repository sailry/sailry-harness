//! ZIP imports are staged separately from projects and never execute package code.
use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::MAX_PACKAGE_BYTES;
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};
#[cfg(test)]
mod tests;

impl Host {
    pub(crate) fn inspect_archive(
        &self,
        file: File,
        stop: CancellationToken,
    ) -> Result<Info, Fault> {
        let (temporary, source) = unpack(file, &stop)?;
        let directory = crate::files::path::root(&source)?;
        let mut tree = package::Tree::default();
        tree.stop = stop;
        tree.walk(&directory, None, "", 0)?;
        let bytes = package::read(&directory, "plugin.json", 64 * 1024)?
            .ok_or_else(|| invalid("plugin.json is required"))?;
        let info = describe(
            &directory,
            manifest::parse(&bytes)?,
            tree.digest(),
            tree.issues,
        )?;
        temporary.close().map_err(io_error)?;
        Ok(info)
    }

    pub(crate) fn install_archive(&self, file: File, name: &str) -> Result<Info, Fault> {
        let (temporary, source) = unpack(file, &CancellationToken::new())?;
        let result = self.install(&source, "", name);
        temporary.close().map_err(io_error)?;
        result
    }
}

fn unpack(file: File, stop: &CancellationToken) -> Result<(tempfile::TempDir, PathBuf), Fault> {
    let (temporary, source, links) = unpack_source(file, stop, 1024)?;
    if !links.is_empty() {
        return Err(invalid("unsupported plugin ZIP entry"));
    }
    if !source.join("plugin.json").is_file() {
        return Err(invalid("plugin.json is required at the package root"));
    }
    Ok((temporary, source))
}

/// Extracts regular files and reports omitted symbolic links relative to the returned root.
pub(super) fn unpack_source(
    mut file: File,
    stop: &CancellationToken,
    max_entries: usize,
) -> Result<(tempfile::TempDir, PathBuf, Vec<String>), Fault> {
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| invalid("invalid plugin ZIP"))?;
    if archive.len() > max_entries {
        return Err(invalid("plugin ZIP has too many entries"));
    }
    let temporary = tempfile::tempdir().map_err(io_error)?;
    let mut total = 0u64;
    let mut names = BTreeSet::new();
    let mut links = Vec::new();
    for index in 0..archive.len() {
        if stop.is_cancelled() {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "plugin inspection cancelled",
            ));
        }
        let mut entry = archive
            .by_index(index)
            .map_err(|_| invalid("invalid plugin ZIP entry"))?;
        let name = entry.name().trim_end_matches('/').to_owned();
        let parts = crate::files::path::components(&name, false)?;
        if parts.len() > 32
            || !names.insert(name.clone())
            || entry.enclosed_name().is_none()
            || entry.encrypted()
            || (!entry.is_symlink()
                && entry
                    .unix_mode()
                    .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o040000 | 0o100000)))
        {
            return Err(invalid("unsupported plugin ZIP entry"));
        }
        // Source repositories may contain unrelated links. Never create or follow them;
        // callers reject links belonging to the package they are importing.
        if entry.is_symlink() {
            links.push(name);
            continue;
        }
        let destination = parts
            .iter()
            .fold(temporary.path().to_path_buf(), |path, part| path.join(part));
        if entry.is_dir() {
            std::fs::create_dir_all(&destination).map_err(io_error)?;
            continue;
        }
        total = total
            .checked_add(entry.size())
            .filter(|total| *total <= MAX_PACKAGE_BYTES)
            .ok_or_else(|| invalid("plugin ZIP exceeds the expanded size limit"))?;
        std::fs::create_dir_all(destination.parent().unwrap()).map_err(io_error)?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(io_error)?;
        let mut copied = 0u64;
        let mut buffer = [0; 64 * 1024];
        loop {
            if stop.is_cancelled() {
                return Err(Fault::new(
                    ErrorCode::Cancelled,
                    "plugin inspection cancelled",
                ));
            }
            let count = entry
                .read(&mut buffer)
                .map_err(|_| invalid("invalid plugin ZIP data"))?;
            if count == 0 {
                break;
            }
            copied += count as u64;
            if copied > entry.size() {
                return Err(invalid("plugin ZIP size mismatch"));
            }
            output.write_all(&buffer[..count]).map_err(io_error)?;
        }
        if copied != entry.size() {
            return Err(invalid("plugin ZIP size mismatch"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let executable = entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0);
            output
                .set_permissions(std::fs::Permissions::from_mode(if executable {
                    0o700
                } else {
                    0o600
                }))
                .map_err(io_error)?;
        }
    }
    let mut source = temporary.path().to_path_buf();
    if !source.join("plugin.json").is_file() {
        let children = std::fs::read_dir(&source)
            .map_err(io_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(io_error)?;
        if children.len() == 1 && children[0].file_type().map_err(io_error)?.is_dir() {
            source = children[0].path();
        }
    }
    let prefix = source
        .strip_prefix(temporary.path())
        .map_err(|_| invalid("source ZIP root is unavailable"))?;
    let links = links
        .into_iter()
        .map(|path| {
            Path::new(&path)
                .strip_prefix(prefix)
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .map_err(|_| invalid("source ZIP contains multiple roots"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let source = source.canonicalize().map_err(io_error)?;
    Ok((temporary, source, links))
}
