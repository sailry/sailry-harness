use super::*;

#[frb(opaque)]
pub struct Upload {
    pub(super) writer: Mutex<Option<DuplexStream>>,
    pub(super) transfer: Transfer,
}

impl Upload {
    pub(in crate::api) fn new(
        client: Arc<Client>,
        upload: sailry_protocol::attachment::Upload,
        stop: CancellationToken,
    ) -> Self {
        let (writer, mut reader) = tokio::io::duplex(FILE_TRANSFER_CHUNK_BYTES);
        let cancellation = stop.clone();
        let transfer = Transfer::spawn(stop, async move {
            client
                .upload_attachment(&upload, &mut reader, cancellation, |_| {})
                .await
        });
        Self {
            writer: Mutex::new(Some(writer)),
            transfer,
        }
    }

    /// Await each write in source order. Accepted bytes are not yet verified or published.
    pub async fn write(&self, bytes: Vec<u8>) -> Result<(), String> {
        check_chunk(&bytes)?;
        tokio::select! {
            biased;
            _ = self.transfer.stop.cancelled() => Err(closed()),
            result = self.transfer.result() => Err(result.err().unwrap_or_else(closed)),
            result = async {
                let mut writer = self.writer.lock().await;
                writer.as_mut().ok_or_else(closed)?.write_all(&bytes).await.map_err(error)
            } => result,
        }
    }

    /// Signal EOF and await the shared Client's complete size/hash verification.
    /// Then submit FinishAttachmentUpload with one retained durable request ID.
    pub async fn finish(&self) -> Result<(), String> {
        tokio::select! {
            biased;
            _ = self.transfer.stop.cancelled() => return Err(closed()),
            mut writer = self.writer.lock() => { writer.take(); }
        }
        self.transfer.result().await
    }

    pub fn close(&self) {
        self.transfer.stop.cancel();
    }
}
