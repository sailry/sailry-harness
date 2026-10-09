use super::plan::{self, Asset};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, Write},
    path::Path,
};

const MAX_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_FILES: usize = 100_000;

pub(super) fn extract(
    mut file: File,
    asset: &Asset,
    root: &Path,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    file.rewind().map_err(invalid)?;
    let directory = root.join(&asset.directory);
    std::fs::create_dir_all(&directory).map_err(invalid)?;
    let mut count = 0usize;
    let mut bytes = 0u64;
    if asset.name.ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(file).map_err(invalid)?;
        if archive.len() > MAX_FILES {
            return Err(Fault::invalid(
                "language archive exceeds the file count limit",
            ));
        }
        for index in 0..archive.len() {
            check(stop)?;
            let mut entry = archive.by_index(index).map_err(invalid)?;
            let name = entry.name().trim_end_matches('/');
            plan::relative(name)?;
            let mode = entry.unix_mode().unwrap_or(0o644);
            if mode & 0o170000 == 0o120000 {
                return Err(Fault::invalid(
                    "language archive contains an unsupported symbolic link",
                ));
            }
            let path = directory.join(name);
            if entry.is_dir() {
                std::fs::create_dir_all(path).map_err(invalid)?;
            } else {
                write(&mut entry, &path, &mut bytes, stop)?;
                executable(&path, mode & 0o777)?;
            }
        }
    } else if asset.name.ends_with(".tar.gz") || asset.name.ends_with(".tgz") {
        tar(
            flate2::read::GzDecoder::new(file),
            &directory,
            &mut count,
            &mut bytes,
            stop,
        )?;
    } else if asset.name.ends_with(".tar.xz") {
        let stream =
            xz2::stream::Stream::new_stream_decoder(256 * 1024 * 1024, 0).map_err(invalid)?;
        tar(
            xz2::read::XzDecoder::new_stream(file, stream),
            &directory,
            &mut count,
            &mut bytes,
            stop,
        )?;
    } else {
        let name = asset.name.strip_suffix(".gz").unwrap_or(&asset.name);
        plan::component(name)?;
        let path = directory.join(name);
        if asset.name.ends_with(".gz") {
            write(
                &mut flate2::read::GzDecoder::new(file),
                &path,
                &mut bytes,
                stop,
            )?;
        } else {
            write(&mut file, &path, &mut bytes, stop)?;
        }
        executable(&path, 0o755)?;
    }
    Ok(())
}

fn tar(
    reader: impl Read,
    directory: &Path,
    count: &mut usize,
    bytes: &mut u64,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries().map_err(invalid)? {
        check(stop)?;
        *count += 1;
        if *count > MAX_FILES {
            return Err(Fault::invalid(
                "language archive exceeds the file count limit",
            ));
        }
        let mut entry = entry.map_err(invalid)?;
        let relative = entry
            .path()
            .map_err(invalid)?
            .to_str()
            .ok_or_else(|| Fault::invalid("language archive has a non-Unicode path"))?
            .trim_end_matches('/')
            .to_owned();
        // A leading ./ is conventional in release archives; it adds no directory access.
        let relative = relative.trim_start_matches("./");
        if relative.is_empty() && entry.header().entry_type().is_dir() {
            continue;
        }
        plan::relative(relative)?;
        let path = directory.join(relative);
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(path).map_err(invalid)?;
        } else if entry.header().entry_type().is_file() {
            let mode = entry.header().mode().map_err(invalid)?;
            write(&mut entry, &path, bytes, stop)?;
            executable(&path, mode & 0o777)?;
        } else {
            return Err(Fault::invalid(
                "language archive contains an unsupported link or special file",
            ));
        }
    }
    Ok(())
}

fn write(
    reader: &mut impl Read,
    path: &Path,
    bytes: &mut u64,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(invalid)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(invalid)?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        check(stop)?;
        let count = reader.read(&mut buffer).map_err(invalid)?;
        if count == 0 {
            break;
        }
        *bytes = bytes
            .checked_add(count as u64)
            .filter(|bytes| *bytes <= MAX_BYTES)
            .ok_or_else(|| Fault::invalid("language archive exceeds the expanded size limit"))?;
        file.write_all(&buffer[..count]).map_err(invalid)?;
    }
    Ok(())
}

pub(super) fn executable(path: &Path, mode: u32) -> Result<(), Fault> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(invalid)?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}
fn check(stop: &CancellationToken) -> Result<(), Fault> {
    if stop.is_cancelled() {
        Err(Fault::new(
            ErrorCode::Cancelled,
            "language extraction was cancelled",
        ))
    } else {
        Ok(())
    }
}
fn invalid(error: impl std::fmt::Display) -> Fault {
    Fault::invalid(error.to_string())
}

#[cfg(test)]
mod tests;
