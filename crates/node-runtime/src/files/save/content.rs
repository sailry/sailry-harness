use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::FILE_TRANSFER_CHUNK_BYTES;

pub(in crate::files) fn revision(
    file: &mut File,
    limit: u64,
    cancel: Option<&CancellationToken>,
) -> Result<String, Fault> {
    let before = file.metadata().map_err(io_error)?;
    if before.len() > limit {
        return Err(conflict());
    }
    let mut hash = blake3::Hasher::new();
    let mut total = 0u64;
    let mut bytes = [0; FILE_TRANSFER_CHUNK_BYTES];
    loop {
        if cancel.is_some_and(CancellationToken::is_cancelled) {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "file inspection cancelled",
            ));
        }
        let count = file.read(&mut bytes).map_err(io_error)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .filter(|size| *size <= before.len())
            .ok_or_else(conflict)?;
        hash.update(&bytes[..count]);
    }
    let after = file.metadata().map_err(io_error)?;
    if total != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(conflict());
    }
    Ok(hash.finalize().to_hex().to_string())
}

pub(in crate::files) fn copy(
    source: &mut impl Read,
    target: &mut impl Write,
    size: u64,
    revision: &str,
) -> Result<(), Fault> {
    let mut hash = blake3::Hasher::new();
    let mut total = 0u64;
    let mut bytes = [0; FILE_TRANSFER_CHUNK_BYTES];
    loop {
        let count = source.read(&mut bytes).map_err(io_error)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .filter(|total| *total <= size)
            .ok_or_else(conflict)?;
        target.write_all(&bytes[..count]).map_err(io_error)?;
        hash.update(&bytes[..count]);
    }
    if total != size || hash.finalize().to_hex().as_str() != revision {
        return Err(conflict());
    }
    Ok(())
}
