use super::*;

pub(in crate::api) enum Source {
    Attachment(sailry_protocol::attachment::Download),
    File(sailry_protocol::FileDownload),
}

#[frb(opaque)]
pub struct Download {
    pub(super) reader: Mutex<DuplexStream>,
    pub(super) transfer: Transfer,
}

impl Download {
    pub(in crate::api) fn new(
        client: Arc<Client>,
        source: Source,
        stop: CancellationToken,
    ) -> Self {
        let (mut writer, reader) = tokio::io::duplex(FILE_TRANSFER_CHUNK_BYTES);
        let cancellation = stop.clone();
        let transfer = Transfer::spawn(stop, async move {
            match source {
                Source::Attachment(download) => {
                    client
                        .download_attachment(&download, &mut writer, cancellation, |_| {})
                        .await
                }
                Source::File(download) => {
                    client
                        .download(&download, &mut writer, cancellation, |_| {})
                        .await
                }
            }
        });
        Self {
            reader: Mutex::new(reader),
            transfer,
        }
    }

    /// Read up to 64 KiB. Only a successful None verifies the full download;
    /// discard partial bytes on any error. Await calls in order.
    pub async fn next(&self) -> Result<Option<Vec<u8>>, String> {
        tokio::select! {
            biased;
            _ = self.transfer.stop.cancelled() => Err(closed()),
            result = async {
                let mut reader = self.reader.lock().await;
                let mut bytes = vec![0; FILE_TRANSFER_CHUNK_BYTES];
                let count = reader.read(&mut bytes).await.map_err(error)?;
                if count == 0 {
                    self.transfer.result().await?;
                    Ok(None)
                } else {
                    bytes.truncate(count);
                    Ok(Some(bytes))
                }
            } => result,
        }
    }

    pub fn close(&self) {
        self.transfer.stop.cancel();
    }
}
