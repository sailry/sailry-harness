//! Tar adapters feed the existing package ZIP validator instead of extracting paths.
use super::*;
use std::io::Write;

pub(super) fn zip(
    mut file: File,
    format: ArchiveFormat,
    stop: &CancellationToken,
) -> Result<File, Fault> {
    file.rewind().map_err(io_error)?;
    match format {
        ArchiveFormat::Zip => Ok(file),
        ArchiveFormat::Tar => convert(file, stop),
        ArchiveFormat::TarGz => convert(flate2::read::GzDecoder::new(file), stop),
    }
}

fn convert(reader: impl Read, stop: &CancellationToken) -> Result<File, Fault> {
    let mut source = tar::Archive::new(reader);
    let mut zip = zip::ZipWriter::new(tempfile::tempfile().map_err(io_error)?);
    let mut total = 0u64;
    let mut names = std::collections::BTreeSet::new();
    for entry in source.entries().map_err(io_error)? {
        if stop.is_cancelled() {
            return Err(cancelled());
        }
        let mut entry = entry.map_err(io_error)?;
        let path = entry
            .path()
            .map_err(io_error)?
            .to_str()
            .ok_or_else(|| Fault::invalid("skill archive paths must be UTF-8"))?
            .to_owned();
        let path = path
            .strip_prefix("./")
            .unwrap_or(&path)
            .trim_end_matches('/');
        if path.is_empty() && entry.header().entry_type().is_dir() {
            continue;
        }
        crate::files::path::components(path, false)?;
        if !names.insert(path.to_owned()) || names.len() > 10_000 {
            return Err(Fault::invalid(
                "skill archive contains duplicate or excessive entries",
            ));
        }
        if entry.header().entry_type().is_dir() {
            zip.add_directory(format!("{path}/"), zip::write::SimpleFileOptions::default())
                .map_err(|_| Fault::invalid("invalid skill archive directory"))?;
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(Fault::invalid(
                "skill archive contains an unsupported link or special file",
            ));
        }
        total = total
            .checked_add(entry.size())
            .filter(|size| *size <= sailry_protocol::plugin::MAX_PACKAGE_BYTES)
            .ok_or_else(|| Fault::invalid("skill archive exceeds the expanded size limit"))?;
        let mode = entry.header().mode().map_err(io_error)? & 0o777;
        zip.start_file(
            path,
            zip::write::SimpleFileOptions::default().unix_permissions(mode),
        )
        .map_err(|_| Fault::invalid("invalid skill archive entry"))?;
        let mut copied = 0u64;
        let mut buffer = [0; 64 * 1024];
        loop {
            if stop.is_cancelled() {
                return Err(cancelled());
            }
            let count = entry.read(&mut buffer).map_err(io_error)?;
            if count == 0 {
                break;
            }
            copied += count as u64;
            if copied > entry.size() {
                return Err(Fault::invalid("skill archive entry changed"));
            }
            zip.write_all(&buffer[..count]).map_err(io_error)?;
        }
        if copied != entry.size() {
            return Err(Fault::invalid("skill archive entry is incomplete"));
        }
    }
    let mut file = zip
        .finish()
        .map_err(|_| Fault::invalid("invalid skill archive"))?;
    file.rewind().map_err(io_error)?;
    Ok(file)
}
