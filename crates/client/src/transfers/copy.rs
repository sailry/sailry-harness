use super::*;
use sailry_protocol::{FileUploadSpec, Output, Request};

impl Client {
    /// Copies a prepared source into destination staging through a bounded pipe.
    /// Both Clients retain their own authenticated Node and caller identity. The
    /// destination spec must match the source size/hash; its expected revision
    /// controls replacement in the same way as an ordinary upload.
    ///
    /// This does not publish or delete anything. Retain the returned stable
    /// FinishFileUpload request before dispatching it, and reuse it after an
    /// uncertain result. A failed/cancelled copy releases both temporary streams;
    /// dropping this future closes active streams and unclaimed resources expire.
    pub async fn stage_copy(
        &self,
        source: &Client,
        download: &FileDownload,
        spec: FileUploadSpec,
        cancel: CancellationToken,
        progress: impl FnMut(u64),
    ) -> Result<Request, Fault> {
        let prepared = async {
            if cancel.is_cancelled() {
                return Err(cancelled());
            }
            if spec.size != download.size || spec.revision != download.revision {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "copy destination must match the source size and revision",
                ));
            }
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(cancelled()),
                result = self.execute(self.prepare(Command::UploadFile(spec.clone()))) => result?,
            };
            let Output::FileUpload(upload) = result else {
                return Err(invalid_response());
            };
            if upload.spec != spec {
                self.cancel_transfer(upload.stream).await;
                return Err(invalid_response());
            }
            Ok(upload)
        }
        .await;
        let upload = match prepared {
            Ok(upload) => upload,
            Err(error) => {
                source.cancel_transfer(download.stream).await;
                return Err(error);
            }
        };

        let (mut reader, mut writer) = tokio::io::duplex(FILE_TRANSFER_CHUNK_BYTES);
        let sending = cancel.clone();
        let receiving = cancel.clone();
        let receive = async move {
            // Owning the pipe end delivers EOF only after source verification,
            // and closes it if either transfer fails or this future is dropped.
            source
                .download(download, &mut writer, receiving, |_| {})
                .await
        };
        let send = async { self.upload(&upload, &mut reader, sending, progress).await };
        let result = tokio::try_join!(receive, send);
        if result.is_err() || cancel.is_cancelled() {
            tokio::join!(
                source.cancel_transfer(download.stream),
                self.cancel_transfer(upload.stream)
            );
            return Err(result.err().unwrap_or_else(cancelled));
        }
        Ok(self.prepare(Command::FinishFileUpload {
            worktree: spec.worktree,
            path: spec.path,
            stream: upload.stream,
        }))
    }
}

fn invalid_response() -> Fault {
    Fault::new(ErrorCode::Internal, "invalid copy staging response")
}
