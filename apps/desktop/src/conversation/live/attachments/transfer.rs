use super::source::Source;
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, ErrorCode, Output, Request, StreamId, WorktreeId,
    attachment::{Attachment, Spec},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::watch;

#[derive(Clone)]
pub(super) enum Status {
    Staged,
    Preparing,
    Sending { copied: u64, size: u64 },
    Publishing(Box<Pending>),
    Ready(Attachment),
    Failed(&'static str),
    Uncertain(Box<Pending>),
    Rejected(Box<Pending>),
    Removing,
    Removed,
}

#[derive(Clone)]
pub(super) struct Pending {
    pub request: Request,
    expected: Spec,
}

pub(super) async fn run(
    client: Arc<Client>,
    worktree: WorktreeId,
    source: Source,
    cancel: CancellationToken,
    updates: watch::Sender<Status>,
    slots: Arc<tokio::sync::Semaphore>,
) -> Status {
    let _permit = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Status::Failed("files_upload_cancelled"),
        permit = slots.acquire_owned() => match permit {
            Ok(permit) => permit,
            Err(_) => return Status::Failed("files_upload_failed"),
        },
    };
    match stage(&client, worktree, source, &cancel, &updates).await {
        Ok(pending) => {
            if cancel.is_cancelled() {
                if let Command::FinishAttachmentUpload { stream, .. } = pending.request.command {
                    release(&client, stream).await;
                }
                return Status::Failed("files_upload_cancelled");
            }
            updates.send_replace(Status::Publishing(Box::new(pending.clone())));
            check(client, pending).await
        }
        Err(key) => Status::Failed(key),
    }
}

async fn stage(
    client: &Client,
    worktree: WorktreeId,
    source: Source,
    cancel: &CancellationToken,
    updates: &watch::Sender<Status>,
) -> Result<Pending, &'static str> {
    let source = source.prepare(worktree, cancel.clone()).await?;
    let spec = source.spec;
    let output = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err("files_upload_cancelled"),
        output = client.execute(client.prepare(Command::UploadAttachment(spec.clone()))) => output,
    };
    let prepared = match output {
        Ok(Output::AttachmentUpload(upload)) if upload.spec == spec => upload,
        Ok(Output::AttachmentUpload(upload)) => {
            release(client, upload.stream).await;
            return Err("files_upload_failed");
        }
        Err(error) if error.code == ErrorCode::Cancelled => return Err("files_upload_cancelled"),
        _ => return Err("files_upload_failed"),
    };
    let mut reader = source.reader;
    let mut last = Instant::now();
    client
        .upload_attachment(&prepared, &mut reader, cancel.clone(), |copied| {
            if copied == 0 || copied == spec.size || last.elapsed() >= Duration::from_millis(75) {
                updates.send_replace(Status::Sending {
                    copied,
                    size: spec.size,
                });
                last = Instant::now();
            }
        })
        .await
        .map_err(|error| match error.code {
            ErrorCode::Cancelled => "files_upload_cancelled",
            ErrorCode::RevisionConflict => "files_upload_changed",
            _ => "files_upload_failed",
        })?;
    Ok(Pending {
        request: client.prepare(Command::FinishAttachmentUpload {
            worktree,
            stream: prepared.stream,
        }),
        expected: spec,
    })
}

pub(super) fn discard(client: &Client, attachment: &Attachment) -> Pending {
    Pending {
        request: client.prepare(Command::DiscardAttachment {
            worktree: attachment.spec.worktree,
            attachment: attachment.id,
        }),
        expected: attachment.spec.clone(),
    }
}

pub(super) async fn check(client: Arc<Client>, pending: Pending) -> Status {
    let removing = matches!(pending.request.command, Command::DiscardAttachment { .. });
    match client.execute(pending.request.clone()).await {
        Ok(Output::Attachment(attachment)) if !removing && attachment.spec == pending.expected => {
            Status::Ready(attachment)
        }
        Ok(Output::AttachmentDiscarded { attachment }) if matches!(pending.request.command, Command::DiscardAttachment { attachment: id, .. } if id == attachment) => {
            Status::Removed
        }
        Err(error)
            if removing && matches!(error.code, ErrorCode::NotFound | ErrorCode::Conflict) =>
        {
            Status::Removed
        }
        Err(error)
            if matches!(
                error.code,
                ErrorCode::Conflict
                    | ErrorCode::RevisionConflict
                    | ErrorCode::InvalidRequest
                    | ErrorCode::PermissionDenied
                    | ErrorCode::NotFound
                    | ErrorCode::WrongTarget
                    | ErrorCode::Busy
            ) && !removing =>
        {
            Status::Failed("files_upload_failed")
        }
        Err(error)
            if removing
                && !matches!(
                    error.code,
                    ErrorCode::OutcomeUnknown | ErrorCode::Unavailable | ErrorCode::Internal
                ) =>
        {
            Status::Rejected(Box::new(pending))
        }
        // Retain the original request whenever durable completion is uncertain.
        _ => Status::Uncertain(Box::new(pending)),
    }
}

async fn release(client: &Client, stream: StreamId) {
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        client.execute(client.prepare(Command::CancelFileTransfer { stream })),
    )
    .await;
}
