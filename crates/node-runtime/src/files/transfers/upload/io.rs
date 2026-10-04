use super::*;
use sailry_protocol::FILE_UPLOAD_READY;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, DuplexStream};

pub(in crate::files::transfers) async fn receive(
    prepared: Box<Prepared>,
    stream: &mut DuplexStream,
) -> Result<Box<Prepared>, Fault> {
    let Prepared {
        file,
        size: expected_size,
        revision,
        target,
        scope,
        _permit,
    } = *prepared;
    let mut file = tokio::fs::File::from_std(file);
    let mut hash = blake3::Hasher::new();
    let mut size = 0u64;
    let mut bytes = [0; FILE_TRANSFER_CHUNK_BYTES];
    loop {
        let count = tokio::time::timeout(IDLE_TIMEOUT, stream.read(&mut bytes))
            .await
            .map_err(|_| unavailable())?
            .map_err(io_error)?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .filter(|size| *size <= expected_size)
            .ok_or_else(changed)?;
        tokio::time::timeout(IDLE_TIMEOUT, file.write_all(&bytes[..count]))
            .await
            .map_err(|_| unavailable())?
            .map_err(io_error)?;
        hash.update(&bytes[..count]);
    }
    if size != expected_size || hash.finalize().to_hex().as_str() != revision {
        return Err(changed());
    }
    tokio::time::timeout(IDLE_TIMEOUT, async {
        file.flush().await?;
        file.sync_all().await?;
        file.seek(std::io::SeekFrom::Start(0)).await?;
        Ok::<_, std::io::Error>(())
    })
    .await
    .map_err(|_| unavailable())?
    .map_err(io_error)?;
    Ok(Box::new(Prepared {
        file: file.into_std().await,
        size,
        revision,
        target,
        scope,
        _permit,
    }))
}

pub(in crate::files::transfers) async fn acknowledge(
    stream: &mut DuplexStream,
) -> Result<(), Fault> {
    tokio::time::timeout(IDLE_TIMEOUT, async {
        stream.write_all(&[FILE_UPLOAD_READY]).await?;
        stream.shutdown().await
    })
    .await
    .map_err(|_| unavailable())?
    .map_err(io_error)
}

fn changed() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "upload size or content changed",
    )
}
