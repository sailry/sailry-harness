use crate::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, FILE_TRANSFER_CHUNK_BYTES, Fault, FileDownload};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};

const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

mod copy;
mod moving;
mod upload;
pub use moving::FileMove;

impl Client {
    /// Receives a verified SSH download into caller-owned staging.
    pub async fn download_ssh(
        &self,
        download: &sailry_protocol::ssh::Download,
        writer: &mut (impl AsyncWrite + Unpin),
        cancel: CancellationToken,
        mut progress: impl FnMut(u64),
    ) -> Result<(), Fault> {
        let result = self
            .receive(
                download.stream,
                download.size,
                &download.revision,
                writer,
                &cancel,
                &mut progress,
            )
            .await;
        if result.is_err() {
            self.cancel_transfer(download.stream).await;
        }
        result
    }

    /// Streams into the writer's current position. The caller owns its staging
    /// file and must publish only after success; any failure can leave partial bytes.
    /// Dropping this future drops the stream, without replaying data or publishing.
    pub async fn download(
        &self,
        download: &FileDownload,
        writer: &mut (impl AsyncWrite + Unpin),
        cancel: CancellationToken,
        mut progress: impl FnMut(u64),
    ) -> Result<(), Fault> {
        let result = self
            .receive(
                download.stream,
                download.size,
                &download.revision,
                writer,
                &cancel,
                &mut progress,
            )
            .await;
        if result.is_err() {
            self.cancel_transfer(download.stream).await;
        }
        result
    }

    /// Uses the same verified stream path as project files; no attachment bytes
    /// or partial destination contents are retained in the Client projection.
    pub async fn download_attachment(
        &self,
        download: &sailry_protocol::attachment::Download,
        writer: &mut (impl AsyncWrite + Unpin),
        cancel: CancellationToken,
        mut progress: impl FnMut(u64),
    ) -> Result<(), Fault> {
        let spec = &download.attachment.spec;
        let result = self
            .receive(
                download.stream,
                spec.size,
                &spec.revision,
                writer,
                &cancel,
                &mut progress,
            )
            .await;
        if result.is_err() {
            self.cancel_transfer(download.stream).await;
        }
        result
    }

    pub(crate) async fn cancel_transfer(&self, stream: sailry_protocol::StreamId) {
        let request = self.prepare(Command::CancelFileTransfer { stream });
        // Best-effort early release. Unclaimed resources also expire on the Node.
        let _ = tokio::time::timeout(Duration::from_secs(2), self.execute(request)).await;
    }

    async fn receive(
        &self,
        resource: sailry_protocol::StreamId,
        size: u64,
        revision: &str,
        writer: &mut (impl AsyncWrite + Unpin),
        cancel: &CancellationToken,
        progress: &mut impl FnMut(u64),
    ) -> Result<(), Fault> {
        let expected = blake3::Hash::from_hex(revision)
            .map_err(|_| Fault::new(ErrorCode::InvalidRequest, "invalid download revision"))?;
        let mut stream = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            result = self.open(resource) => result?,
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            result = tokio::time::timeout(IDLE_TIMEOUT, stream.shutdown()) => result.map_err(|_| stalled())?.map_err(|_| unavailable())?,
        }
        let mut total = 0u64;
        let mut hash = blake3::Hasher::new();
        let mut buffer = vec![0; FILE_TRANSFER_CHUNK_BYTES];
        progress(0);
        loop {
            let count = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(cancelled()),
                result = tokio::time::timeout(IDLE_TIMEOUT, stream.read(&mut buffer)) => result.map_err(|_| stalled())?.map_err(|_| unavailable())?,
            };
            if count == 0 {
                break;
            }
            total = total
                .checked_add(count as u64)
                .filter(|total| *total <= size)
                .ok_or_else(changed)?;
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(cancelled()),
                result = tokio::time::timeout(IDLE_TIMEOUT, writer.write_all(&buffer[..count])) => result.map_err(|_| stalled())?.map_err(|_| Fault::new(ErrorCode::Unavailable, "download destination write failed"))?,
            }
            hash.update(&buffer[..count]);
            progress(total);
        }
        if total != size || hash.finalize() != expected {
            return Err(changed());
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(cancelled()),
            result = tokio::time::timeout(IDLE_TIMEOUT, writer.flush()) => result.map_err(|_| stalled())?.map_err(|_| Fault::new(ErrorCode::Unavailable, "download destination flush failed")),
        }
    }
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "file transfer cancelled")
}
fn stalled() -> Fault {
    Fault::new(ErrorCode::Unavailable, "file transfer made no progress")
}
fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "file transfer stream failed")
}
fn changed() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "file transfer size or content changed; partial data must not be published",
    )
}
