use super::destination::Destination;
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Output, WorktreeId};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::watch;

#[derive(Clone)]
pub(crate) enum Source {
    ScopedSsh {
        context: sailry_protocol::plugin::Context,
        profile: sailry_protocol::ssh::Profile,
        path: String,
    },
    File {
        worktree: WorktreeId,
        path: String,
    },
    ScopedFile {
        context: sailry_protocol::plugin::Context,
        path: String,
    },
    Attachment(sailry_protocol::attachment::Attachment),
    Image {
        session: sailry_protocol::SessionId,
        image: sailry_protocol::conversation::Image,
    },
    LocalPath(PathBuf),
    Bytes(Arc<[u8]>),
}

#[derive(Clone)]
pub(crate) enum Status {
    Preparing,
    Receiving { copied: u64, size: u64 },
    Publishing,
    Finished(Result<(), &'static str>),
}

pub(crate) async fn run(
    client: Arc<Client>,
    source: Source,
    destination: PathBuf,
    cancel: CancellationToken,
    updates: watch::Sender<Status>,
) {
    let result = receive(client, source, destination, cancel, &updates).await;
    updates.send_replace(Status::Finished(result));
}

async fn receive(
    client: Arc<Client>,
    source: Source,
    destination: PathBuf,
    cancel: CancellationToken,
    updates: &watch::Sender<Status>,
) -> Result<(), &'static str> {
    let destination = tokio::task::spawn_blocking(move || Destination::prepare(destination))
        .await
        .map_err(|_| "files_download_destination")??;
    let mut writer = tokio::fs::File::from_std(destination.writer()?);
    if matches!(source, Source::LocalPath(_) | Source::Bytes(_)) {
        receive_local(source, &mut writer, cancel.clone(), updates).await?;
        drop(writer);
        return publish(destination, cancel, updates).await;
    }
    let request = client.prepare(match &source {
        Source::ScopedSsh { profile, path, .. } => Command::DownloadSshFile {
            profile: profile.id,
            expected_revision: profile.revision,
            path: path.clone(),
        },
        Source::File { worktree, path } => Command::DownloadFile {
            worktree: *worktree,
            path: path.clone(),
        },
        Source::ScopedFile { context, path } => Command::DownloadFile {
            worktree: context.worktree.ok_or("files_download_failed")?,
            path: path.clone(),
        },
        Source::Attachment(attachment) => Command::DownloadAttachment {
            worktree: attachment.spec.worktree,
            attachment: attachment.id,
        },
        Source::Image { session, image } => Command::DownloadImage {
            session: *session,
            image: image.clone(),
        },
        Source::LocalPath(_) | Source::Bytes(_) => unreachable!("local download was handled"),
    });
    let request = match &source {
        Source::ScopedFile { context, .. } | Source::ScopedSsh { context, .. } => {
            request.with_plugin(context.clone())
        }
        _ => request,
    };
    let output = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err("files_download_cancelled"),
        output = client.execute(request) => output.map_err(|_| "files_download_failed")?,
    };
    let (valid, stream, size) = match (&source, &output) {
        (
            Source::ScopedSsh { path, .. },
            Output::SshOutcome(sailry_protocol::ssh::Outcome::Download(download)),
        ) => (download.path == *path, download.stream, download.size),
        (Source::File { worktree, path }, Output::FileDownload(download)) => (
            download.worktree == *worktree && download.path == *path,
            download.stream,
            download.size,
        ),
        (Source::ScopedFile { context, path }, Output::FileDownload(download)) => (
            Some(download.worktree) == context.worktree && download.path == *path,
            download.stream,
            download.size,
        ),
        (Source::Attachment(attachment), Output::AttachmentDownload(download)) => (
            &download.attachment == attachment,
            download.stream,
            download.attachment.spec.size,
        ),
        (Source::Image { image, .. }, Output::AttachmentDownload(download)) => (
            download.attachment == image.attachment,
            download.stream,
            download.attachment.spec.size,
        ),
        _ => return Err("files_download_failed"),
    };
    if !valid {
        let _ = client
            .execute(client.prepare(Command::CancelFileTransfer { stream }))
            .await;
        return Err("files_download_failed");
    }
    let mut last = Instant::now();
    let progress = |copied| {
        if copied == 0 || copied == size || last.elapsed() >= Duration::from_millis(75) {
            updates.send_replace(Status::Receiving { copied, size });
            last = Instant::now();
        }
    };
    let result = match output {
        Output::SshOutcome(sailry_protocol::ssh::Outcome::Download(download)) => {
            client
                .download_ssh(&download, &mut writer, cancel.clone(), progress)
                .await
        }
        Output::FileDownload(download) => {
            client
                .download(&download, &mut writer, cancel.clone(), progress)
                .await
        }
        Output::AttachmentDownload(download) => {
            client
                .download_attachment(&download, &mut writer, cancel.clone(), progress)
                .await
        }
        _ => unreachable!("download descriptor was checked"),
    };
    drop(writer);
    result.map_err(|fault| match fault.code {
        ErrorCode::Cancelled => "files_download_cancelled",
        ErrorCode::RevisionConflict => "files_download_changed",
        _ => "files_download_failed",
    })?;
    publish(destination, cancel, updates).await
}

async fn publish(
    destination: Destination,
    cancel: CancellationToken,
    updates: &watch::Sender<Status>,
) -> Result<(), &'static str> {
    updates.send_replace(Status::Publishing);
    tokio::task::spawn_blocking(move || destination.publish(&cancel))
        .await
        .map_err(|_| "files_download_destination")?
}

async fn receive_local(
    source: Source,
    writer: &mut tokio::fs::File,
    cancel: CancellationToken,
    updates: &watch::Sender<Status>,
) -> Result<(), &'static str> {
    use std::io::Read as _;
    use tokio::io::AsyncWriteExt as _;
    let stopping = cancel.clone();
    let bytes: Arc<[u8]> = tokio::task::spawn_blocking(move || {
        if stopping.is_cancelled() {
            return Err("files_download_cancelled");
        }
        match source {
            Source::Bytes(bytes) => Ok(bytes),
            Source::LocalPath(path) => {
                let source = crate::resources::file_source::Source::open_limited(
                    &path,
                    &stopping,
                    sailry_protocol::attachment::MAX_BYTES,
                )?;
                let mut bytes = Vec::new();
                source
                    .file
                    .take(source.size + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "files_download_failed")?;
                if bytes.len() as u64 != source.size
                    || blake3::hash(&bytes).to_hex().as_str() != source.revision
                {
                    return Err("files_download_changed");
                }
                Ok(bytes.into())
            }
            _ => unreachable!("local source expected"),
        }
    })
    .await
    .map_err(|_| "files_download_failed")??;
    let size = bytes.len() as u64;
    let mut copied = 0;
    for chunk in bytes.chunks(sailry_protocol::FILE_TRANSFER_CHUNK_BYTES) {
        if cancel.is_cancelled() {
            return Err("files_download_cancelled");
        }
        writer
            .write_all(chunk)
            .await
            .map_err(|_| "files_download_destination")?;
        copied += chunk.len() as u64;
        updates.send_replace(Status::Receiving { copied, size });
    }
    Ok(())
}
