//! Bounded reading adapted from sailry-platform 727ce0a:
//! harbor-file-service/src/worktree.rs read_transfer_chunk (Apache-2.0).
//! Retain the opened source and verify bytes at the Client, without claiming
//! isolation from external writers or copying entire files into memory.
use super::*;
use crate::files::{Control, io_error, open_regular, path, version};
use std::{
    io::{Read, Seek},
    path::Path,
    time::Instant,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) fn prepare(
    root: &Path,
    relative: &str,
    profile: Option<&Path>,
    cancel: CancellationToken,
    permit: OwnedSemaphorePermit,
) -> Result<Prepared, Fault> {
    let control = Control {
        cancelled: cancel,
        deadline: Instant::now() + PREPARE_TIMEOUT,
    };
    control.check()?;
    let parts = path::components(relative, false)?;
    let (name, parents) = parts.split_last().unwrap();
    let root_dir = path::root(root)?;
    let dir = path::descend(root_dir.try_clone().map_err(io_error)?, parents)?;
    // Business file access cannot expose the execution Node's protected storage.
    if let Some(profile) = profile {
        let source = root.join(relative).canonicalize().map_err(io_error)?;
        if source.starts_with(profile) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "Node storage is not a downloadable file",
            ));
        }
    }
    let mut file = open_regular(&dir, name)?;
    let before = file.metadata().map_err(io_error)?;
    let stamp = version::retained(&root_dir, &dir, &file, relative)?;
    let size = before.len();
    let mut total = 0;
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0; FILE_TRANSFER_CHUNK_BYTES];
    loop {
        control.check()?;
        let limit = size
            .saturating_sub(total)
            .saturating_add(1)
            .min(buffer.len() as u64) as usize;
        let count = Read::read(&mut file, &mut buffer[..limit]).map_err(io_error)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > size {
            return Err(changed());
        }
        hash.update(&buffer[..count]);
    }
    let after = file.metadata().map_err(io_error)?;
    if total != size || after.len() != size || before.modified().ok() != after.modified().ok() {
        return Err(changed());
    }
    control.check()?;
    if version::retained(&root_dir, &dir, &file, relative)? != stamp
        || version::current(root, relative)? != stamp
    {
        return Err(changed());
    }
    file.rewind().map_err(io_error)?;
    Ok(Prepared {
        source: Source::File(file.into_std()),
        size,
        revision: hash.finalize().to_hex().to_string(),
        stamp,
        _permit: permit,
    })
}

pub(super) async fn send(
    prepared: Prepared,
    service: &mut tokio::io::DuplexStream,
) -> std::io::Result<()> {
    let Prepared {
        source,
        size,
        _permit,
        ..
    } = prepared;
    let source: Box<dyn tokio::io::AsyncRead + Unpin + Send> = match source {
        Source::File(file) => Box::new(tokio::fs::File::from_std(file)),
        Source::Bytes(bytes) => Box::new(std::io::Cursor::new(bytes)),
    };
    let mut file = source.take(size);
    let mut buffer = [0; FILE_TRANSFER_CHUNK_BYTES];
    loop {
        let count = tokio::time::timeout(IDLE_TIMEOUT, file.read(&mut buffer)).await??;
        if count == 0 {
            break;
        }
        // On cancellation discard the stream, never retry a partially written block.
        tokio::time::timeout(IDLE_TIMEOUT, service.write_all(&buffer[..count])).await??;
    }
    tokio::time::timeout(IDLE_TIMEOUT, service.shutdown()).await??;
    Ok(())
}

fn changed() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "download source changed while reading",
    )
}
