//! Bounded ZIP snapshots; untrusted archives are never extracted to disk.
use super::*;
use std::io::{Cursor, Read, Write};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

// The payload remains bounded at 32 MiB; ZIP headers and Deflate framing are separate.
const INPUT_LIMIT: usize = resources::PACKAGE_LIMIT + 64 * 1024;
const ENTRY_LIMIT: usize = 256;

#[cfg(test)]
mod tests;

pub(super) fn load(path: &Path, strict: bool) -> Result<Loaded, String> {
    let bytes = resources::read(path, INPUT_LIMIT)?;
    let mut archive =
        ZipArchive::new(Cursor::new(bytes.as_slice())).map_err(|error| error.to_string())?;
    if archive.len() > ENTRY_LIMIT {
        return Err("theme archive exceeds entry limit".into());
    }
    validate_headers(&mut archive, &bytes)?;
    let mut files = BTreeMap::new();
    let mut encoded = 0;
    for index in 0..archive.len() {
        let metadata = archive
            .by_index_raw(index)
            .map_err(|error| error.to_string())?;
        let name = metadata.name().to_string();
        validate_path(&name, metadata.is_dir())?;
        if metadata.is_symlink() || metadata.encrypted() {
            return Err("theme archives cannot contain links or encrypted entries".into());
        }
        let directory = metadata.is_dir();
        let size = metadata.size();
        drop(metadata);
        let limit = if name.rsplit('/').next() == Some("theme.json") {
            resources::MANIFEST_LIMIT
        } else {
            resources::FILE_LIMIT
        };
        if size > limit as u64
            || size > (resources::PACKAGE_LIMIT - encoded) as u64
            || (directory && size != 0)
        {
            return Err("theme archive exceeds resource limit".into());
        }
        let mut file = archive.by_index(index).map_err(|error| error.to_string())?;
        let mut content = Vec::new();
        // Read past the permitted size to distinguish EOF from a decompression bomb.
        file.by_ref()
            .take(limit as u64 + 1)
            .read_to_end(&mut content)
            .map_err(|error| error.to_string())?;
        if content.len() as u64 != size {
            return Err("theme archive resource size does not match its entry".into());
        }
        encoded += content.len();
        if !directory && files.insert(name, content).is_some() {
            return Err("theme archive contains duplicate entries".into());
        }
    }
    let manifests = files
        .keys()
        .filter(|name| name.rsplit('/').next() == Some("theme.json"))
        .collect::<Vec<_>>();
    let [manifest] = manifests.as_slice() else {
        return Err("theme archive requires one theme.json".into());
    };
    let root = manifest.strip_suffix("theme.json").unwrap().to_string();
    if root.trim_end_matches('/').contains('/') {
        return Err("theme archive requires a package root or one containing folder".into());
    }
    let files = files
        .into_iter()
        .filter_map(|(name, bytes)| {
            name.strip_prefix(&root)
                .map(|relative| (relative.to_string(), bytes))
        })
        .collect();
    resources::load_files(files, strict)
}

fn validate_path(path: &str, directory: bool) -> Result<(), String> {
    let path = if directory {
        path.strip_suffix('/')
            .ok_or("invalid theme archive directory")?
    } else {
        path
    };
    if path.is_empty()
        || path.len() > 512
        || path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("theme archive paths must be normalized and relative".into());
    }
    Ok(())
}

fn validate_headers(archive: &mut ZipArchive<Cursor<&[u8]>>, bytes: &[u8]) -> Result<(), String> {
    let mut headers = Vec::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index_raw(index)
            .map_err(|error| error.to_string())?;
        headers.push(file.central_header_start());
        let start =
            usize::try_from(file.header_start()).map_err(|_| "invalid theme archive header")?;
        let header = segment(bytes, start, 30)?;
        if !header.starts_with(b"PK\x03\x04") {
            return Err("invalid theme archive file header".into());
        }
        if u16::from_le_bytes([header[6], header[7]]) & 1 != 0 {
            return Err("theme archives cannot contain encrypted entries".into());
        }
        let length = usize::from(u16::from_le_bytes([header[26], header[27]]));
        if segment(bytes, start + 30, length)? != file.name_raw() {
            return Err("theme archive file names do not match".into());
        }
    }
    headers.sort_unstable();
    let mut position = usize::try_from(archive.central_directory_start())
        .map_err(|_| "invalid theme archive directory")?;
    // zip 8.6.0 collapses duplicate raw names. Check that it retained every
    // central header, using its offsets rather than implementing a ZIP decoder.
    for offset in headers {
        if offset != position as u64 {
            return Err("theme archive contains duplicate or malformed entries".into());
        }
        let header = segment(bytes, position, 46)?;
        if !header.starts_with(b"PK\x01\x02") {
            return Err("invalid theme archive directory header".into());
        }
        let extra = [28, 30, 32]
            .into_iter()
            .map(|index| usize::from(u16::from_le_bytes([header[index], header[index + 1]])))
            .sum::<usize>();
        segment(bytes, position + 46, extra)?;
        position += 46 + extra;
    }
    if bytes
        .get(position..)
        .is_some_and(|tail| tail.starts_with(b"PK\x01\x02"))
    {
        return Err("theme archive contains duplicate entries".into());
    }
    Ok(())
}

fn segment(bytes: &[u8], start: usize, length: usize) -> Result<&[u8], String> {
    let end = start
        .checked_add(length)
        .ok_or("invalid theme archive header")?;
    bytes
        .get(start..end)
        .ok_or_else(|| "invalid theme archive header".into())
}

pub(super) fn export(loaded: &Loaded, path: &Path) -> Result<(), String> {
    if !loaded.warnings.is_empty() {
        return Err("theme package has unverified resources".into());
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut staging = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    let mut archive = ZipWriter::new(staging.as_file_mut());
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for (name, bytes) in &loaded.files {
        archive
            .start_file(name, options)
            .map_err(|error| error.to_string())?;
        archive
            .write_all(bytes)
            .map_err(|error| error.to_string())?;
    }
    archive.finish().map_err(|error| error.to_string())?;
    staging
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    staging.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}
