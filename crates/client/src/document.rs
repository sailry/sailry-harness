//! Complete text access through existing bounded control messages and data streams.
use crate::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, ErrorCode, Fault, FileContent, FileUploadSpec, MAX_FILE_BYTES, Output, Request,
    WorktreeId,
};

/// Restores the text limit of Platform 727ce0a's file service. Larger files keep
/// a read-only preview and remain available through the binary download API.
pub const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;

/// A staged text revision and its stable publication request. Staging is not a
/// save; retain this value before dispatching and reuse it after an uncertain result.
#[derive(Clone, Debug)]
pub struct DocumentWrite {
    pub request: Request,
    spec: FileUploadSpec,
}

impl DocumentWrite {
    pub fn matches(
        &self,
        worktree: WorktreeId,
        path: &str,
        text: &str,
        expected_revision: &str,
    ) -> bool {
        self.spec.worktree == worktree
            && self.spec.path == path
            && self.spec.expected_revision.as_deref() == Some(expected_revision)
            && self.spec.size == text.len() as u64
            && self.spec.revision == blake3::hash(text.as_bytes()).to_hex().as_str()
    }
}

impl Client {
    /// Returns verified complete text up to MAX_DOCUMENT_BYTES, otherwise the
    /// original bounded preview without a writable revision. Cancellation or a
    /// failed transfer never exposes partial bytes as a complete document.
    pub async fn read_document(
        &self,
        worktree: WorktreeId,
        path: &str,
        cancel: CancellationToken,
    ) -> Result<FileContent, Fault> {
        self.read_document_in(worktree, path, cancel, None).await
    }

    pub async fn read_document_scoped(
        &self,
        context: &sailry_protocol::plugin::Context,
        path: &str,
        cancel: CancellationToken,
    ) -> Result<FileContent, Fault> {
        let worktree = scope(context)?;
        self.read_document_in(worktree, path, cancel, Some(context))
            .await
    }

    async fn read_document_in(
        &self,
        worktree: WorktreeId,
        path: &str,
        cancel: CancellationToken,
        context: Option<&sailry_protocol::plugin::Context>,
    ) -> Result<FileContent, Fault> {
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            result = self.execute(prepare(self, Command::ReadFile { worktree, path: path.into() }, context)) => result?,
        };
        let Output::FileContent(content) = result else {
            return Err(invalid_response());
        };
        if content.path != path || content.text.len() > MAX_FILE_BYTES {
            return Err(invalid_response());
        }
        if !content.truncated {
            if content.size != content.text.len() as u64
                || content.text.contains('\0')
                || content.revision.as_deref()
                    != Some(blake3::hash(content.text.as_bytes()).to_hex().as_str())
            {
                return Err(invalid_response());
            }
            return Ok(content);
        }
        if content.revision.is_some() || content.size <= content.text.len() as u64 {
            return Err(invalid_response());
        }
        if content.size > MAX_DOCUMENT_BYTES as u64 {
            return Ok(content);
        }
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            result = self.execute(prepare(self, Command::DownloadFile { worktree, path: path.into() }, context)) => result?,
        };
        let Output::FileDownload(download) = result else {
            return Err(invalid_response());
        };
        if download.worktree != worktree || download.path != path {
            self.cancel_transfer(download.stream).await;
            return Err(invalid_response());
        }
        if download.size > MAX_DOCUMENT_BYTES as u64 {
            self.cancel_transfer(download.stream).await;
            return Ok(content);
        }
        let mut bytes = Vec::with_capacity(download.size as usize);
        self.download(&download, &mut bytes, cancel, |_| {}).await?;
        let text = String::from_utf8(bytes)
            .map_err(|_| Fault::new(ErrorCode::InvalidRequest, "file is not UTF-8 text"))?;
        validate_text(&text)?;
        Ok(FileContent {
            path: download.path,
            text,
            size: download.size,
            truncated: false,
            revision: Some(download.revision),
        })
    }

    /// Prepares a revision-checked text replacement over the authenticated upload
    /// stream, including shrinking a formerly large file. This never publishes it. Execute
    /// the returned stable request separately after retaining it for recovery.
    pub async fn stage_document(
        &self,
        worktree: WorktreeId,
        path: &str,
        text: &str,
        expected_revision: &str,
        cancel: CancellationToken,
    ) -> Result<DocumentWrite, Fault> {
        self.stage_document_in(worktree, path, text, expected_revision, cancel, None)
            .await
    }

    pub async fn stage_document_scoped(
        &self,
        context: &sailry_protocol::plugin::Context,
        path: &str,
        text: &str,
        expected_revision: &str,
        cancel: CancellationToken,
    ) -> Result<DocumentWrite, Fault> {
        let worktree = scope(context)?;
        self.stage_document_in(
            worktree,
            path,
            text,
            expected_revision,
            cancel,
            Some(context),
        )
        .await
    }

    async fn stage_document_in(
        &self,
        worktree: WorktreeId,
        path: &str,
        text: &str,
        expected_revision: &str,
        cancel: CancellationToken,
        context: Option<&sailry_protocol::plugin::Context>,
    ) -> Result<DocumentWrite, Fault> {
        validate_text(text)?;
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        let spec = FileUploadSpec {
            worktree,
            path: path.into(),
            size: text.len() as u64,
            revision: blake3::hash(text.as_bytes()).to_hex().to_string(),
            expected_revision: Some(expected_revision.into()),
        };
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            result = self.execute(prepare(self, Command::UploadFile(spec.clone()), context)) => result?,
        };
        let Output::FileUpload(upload) = result else {
            return Err(invalid_response());
        };
        if upload.spec != spec {
            self.cancel_transfer(upload.stream).await;
            return Err(invalid_response());
        }
        self.upload(&upload, &mut text.as_bytes(), cancel, |_| {})
            .await?;
        Ok(DocumentWrite {
            request: prepare(
                self,
                Command::FinishFileUpload {
                    worktree,
                    path: path.into(),
                    stream: upload.stream,
                },
                context,
            ),
            spec,
        })
    }
}

fn scope(context: &sailry_protocol::plugin::Context) -> Result<WorktreeId, Fault> {
    context.worktree.ok_or_else(|| {
        Fault::new(
            ErrorCode::InvalidRequest,
            "document requires a captured worktree",
        )
    })
}

fn prepare(
    client: &Client,
    command: Command,
    context: Option<&sailry_protocol::plugin::Context>,
) -> Request {
    let request = client.prepare(command);
    match context {
        Some(context) => request.with_plugin(context.clone()),
        None => request,
    }
}

fn validate_text(text: &str) -> Result<(), Fault> {
    if text.len() > MAX_DOCUMENT_BYTES {
        Err(Fault::new(
            ErrorCode::InvalidRequest,
            "document exceeds the text editing limit",
        ))
    } else if text.contains('\0') {
        Err(Fault::new(
            ErrorCode::InvalidRequest,
            "binary file cannot be edited as text",
        ))
    } else {
        Ok(())
    }
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "document operation cancelled")
}

fn invalid_response() -> Fault {
    Fault::new(ErrorCode::Internal, "invalid document response")
}

#[cfg(test)]
mod tests;
