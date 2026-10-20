use super::*;
use sailry_protocol::{FILE_UPLOAD_READY, FileUpload};
use tokio::io::AsyncRead;

/// A Node-admitted staging stream and its exact source digest.
pub trait Upload {
    fn descriptor(&self) -> (sailry_protocol::StreamId, u64, &str);
}

impl Upload for FileUpload {
    fn descriptor(&self) -> (sailry_protocol::StreamId, u64, &str) {
        (self.stream, self.spec.size, &self.spec.revision)
    }
}

impl Upload for sailry_protocol::ssh::Upload {
    fn descriptor(&self) -> (sailry_protocol::StreamId, u64, &str) {
        (self.stream, self.spec.size, &self.spec.revision)
    }
}

impl Upload for sailry_protocol::plugin::Upload {
    fn descriptor(&self) -> (sailry_protocol::StreamId, u64, &str) {
        (self.stream, self.spec.size, &self.spec.revision)
    }
}

impl Upload for sailry_protocol::attachment::Upload {
    fn descriptor(&self) -> (sailry_protocol::StreamId, u64, &str) {
        (self.stream, self.spec.size, &self.spec.revision)
    }
}

impl Upload for sailry_protocol::backup::Upload {
    fn descriptor(&self) -> (sailry_protocol::StreamId, u64, &str) {
        (self.stream, self.size, &self.revision)
    }
}

impl Client {
    /// Stages verified bytes; the resource's durable finish command publishes them.
    /// Retain that command's request ID across reconnects. Failed streams are not replayed.
    pub async fn upload(
        &self,
        upload: &impl Upload,
        reader: &mut (impl AsyncRead + Unpin),
        cancel: CancellationToken,
        mut progress: impl FnMut(u64),
    ) -> Result<(), Fault> {
        let (stream, size, revision) = upload.descriptor();
        let result = self
            .send_upload(stream, size, revision, reader, &cancel, &mut progress)
            .await;
        if result.is_err() {
            self.cancel_transfer(stream).await;
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
            .map_err(|_| Fault::invalid("invalid upload revision"))?;
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
