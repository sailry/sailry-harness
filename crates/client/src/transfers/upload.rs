use super::*;
use sailry_protocol::{FILE_UPLOAD_READY, FileUpload};
use tokio::io::AsyncRead;

impl Client {
    /// Stages SSH bytes; FinishSshUpload separately authorizes remote publication.
    pub async fn upload_ssh(
        &self,
        upload: &sailry_protocol::ssh::Upload,
        reader: &mut (impl AsyncRead + Unpin),
        cancel: CancellationToken,
        mut progress: impl FnMut(u64),
    ) -> Result<(), Fault> {
        let result = self
            .send_upload(
                upload.stream,
                upload.spec.size,
                &upload.spec.revision,
                reader,
                &cancel,
                &mut progress,
            )
            .await;
        if result.is_err() {
            self.cancel_transfer(upload.stream).await;
        }
        result
    }

    /// Stages a plugin ZIP for inspection; installation requires a durable command.
    pub async fn upload_plugin(
        &self,
        upload: &sailry_protocol::plugin::Upload,
        reader: &mut (impl AsyncRead + Unpin),
        cancel: CancellationToken,
        mut progress: impl FnMut(u64),
    ) -> Result<(), Fault> {
        let result = self
            .send_upload(
                upload.stream,
                upload.spec.size,
                &upload.spec.revision,
                reader,
                &cancel,
                &mut progress,
            )
            .await;
        if result.is_err() {
            self.cancel_transfer(upload.stream).await;
        }
        result
    }
    /// Stages verified bytes only. The caller must retain a stable FinishFileUpload
    /// request and await its durable business result before reporting publication.
    /// Failed or dropped streams are never replayed; the source remains caller-owned.
    pub async fn upload(
        &self,
        upload: &FileUpload,
        reader: &mut (impl AsyncRead + Unpin),
        cancel: CancellationToken,
        mut progress: impl FnMut(u64),
    ) -> Result<(), Fault> {
        let result = self
            .send_upload(
                upload.stream,
                upload.spec.size,
                &upload.spec.revision,
                reader,
                &cancel,
                &mut progress,
            )
            .await;
        if result.is_err() {
            self.cancel_transfer(upload.stream).await;
        }
        result
    }

    /// Stages an attachment without publishing metadata or admitting a turn.
    /// FinishAttachmentUpload must retain its original durable request ID.
    pub async fn upload_attachment(
        &self,
        upload: &sailry_protocol::attachment::Upload,
        reader: &mut (impl AsyncRead + Unpin),
        cancel: CancellationToken,
        mut progress: impl FnMut(u64),
    ) -> Result<(), Fault> {
        let result = self
            .send_upload(
                upload.stream,
                upload.spec.size,
                &upload.spec.revision,
                reader,
                &cancel,
                &mut progress,
            )
            .await;
        if result.is_err() {
            self.cancel_transfer(upload.stream).await;
        }
        result
    }

    async fn send_upload(
        &self,
        resource: sailry_protocol::StreamId,
        expected_size: u64,
        revision: &str,
        reader: &mut (impl AsyncRead + Unpin),
        cancel: &CancellationToken,
        progress: &mut impl FnMut(u64),
    ) -> Result<(), Fault> {
        let expected = blake3::Hash::from_hex(revision)
            .map_err(|_| Fault::new(ErrorCode::InvalidRequest, "invalid upload revision"))?;
        let mut stream = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            result = self.open(resource) => result?,
        };
        let mut size = 0u64;
        let mut hash = blake3::Hasher::new();
        let mut bytes = vec![0; FILE_TRANSFER_CHUNK_BYTES];
        progress(0);
        loop {
            let count = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(cancelled()),
                result = tokio::time::timeout(IDLE_TIMEOUT, reader.read(&mut bytes)) => result.map_err(|_| stalled())?.map_err(|_| Fault::new(ErrorCode::Unavailable, "upload source read failed"))?,
            };
            if count == 0 {
                break;
            }
            size = size
                .checked_add(count as u64)
                .filter(|size| *size <= expected_size)
                .ok_or_else(changed)?;
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(cancelled()),
                result = tokio::time::timeout(IDLE_TIMEOUT, stream.write_all(&bytes[..count])) => result.map_err(|_| stalled())?.map_err(|_| unavailable())?,
            }
            hash.update(&bytes[..count]);
            progress(size);
        }
        if size != expected_size || hash.finalize() != expected {
            return Err(changed());
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(cancelled()),
            result = tokio::time::timeout(IDLE_TIMEOUT, async {
                stream.shutdown().await.map_err(|_| unavailable())?;
                let ready = stream.read_u8().await.map_err(|_| unavailable())?;
                if ready != FILE_UPLOAD_READY || stream.read(&mut [0]).await.map_err(|_| unavailable())? != 0 { return Err(unavailable()); }
                Ok(())
            }) => result.map_err(|_| stalled())?,
        }
    }
}
