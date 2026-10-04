//! Remote file operations use SFTP over the existing verified SSH connection.
use super::*;
use crate::files::{io_error, transfers::Transfers};
use russh_sftp::{client::SftpSession, protocol::OpenFlags};
use sailry_protocol::{EntryKind, FileEntry, NodeId, StreamId, ssh::Directory};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

mod mutation;

pub(crate) enum Operation {
    Modify {
        path: String,
        action: sailry_protocol::ssh::FileAction,
    },
    List {
        path: String,
        after: Option<sailry_protocol::ssh::Cursor>,
    },
    Download(String),
    Upload {
        path: String,
        stream: StreamId,
    },
}

pub(crate) fn validate_path(path: &str) -> Result<(), Fault> {
    if path.is_empty() || path.len() > 4096 || path.contains('\0') {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "Invalid SSH file path",
        ));
    }
    Ok(())
}

pub(super) async fn execute(
    session: &client::Handle<Verifier>,
    profile: &Profile,
    operation: Operation,
    caller: NodeId,
    transfers: Arc<Transfers>,
    closed: CancellationToken,
) -> Result<Outcome, Fault> {
    let channel = session
        .channel_open_session()
        .await
        .map_err(|_| unavailable("SSH file channel could not be opened"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|_| unavailable("SFTP is unavailable on the SSH server"))?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(error)?;
    let outcome = match operation {
        Operation::Modify { path, action } => mutation::execute(&sftp, &path, action)
            .await
            .map(|_| Outcome::FilesChanged),
        Operation::List { path, after } => {
            let path = sftp.canonicalize(path).await.map_err(error)?;
            let mut entries = Vec::new();
            for entry in sftp.read_dir(path.clone()).await.map_err(error)? {
                let name = entry.file_name();
                if name.contains('/') || name.contains('\0') {
                    continue;
                }
                let metadata = entry.metadata();
                let kind = if metadata.is_dir() {
                    EntryKind::Directory
                } else if metadata.file_type().is_file() {
                    EntryKind::File
                } else if metadata.is_symlink() {
                    EntryKind::Symlink
                } else {
                    EntryKind::Other
                };
                entries.push(FileEntry {
                    name,
                    kind,
                    size: metadata.len(),
                });
            }
            entries.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
            if let Some(after) = after {
                entries.retain(|entry| entry.sort_key() > (!after.directory, after.name.as_str()));
            }
            let truncated = entries.len() > sailry_protocol::MAX_DIRECTORY_ENTRIES;
            entries.truncate(sailry_protocol::MAX_DIRECTORY_ENTRIES);
            let next = if truncated {
                entries.last().map(|entry| sailry_protocol::ssh::Cursor {
                    directory: entry.kind == EntryKind::Directory,
                    name: entry.name.clone(),
                })
            } else {
                None
            };
            Ok(Outcome::Directory(Directory {
                path,
                entries,
                next,
            }))
        }
        Operation::Download(path) => {
            let mut remote = sftp.open(path.clone()).await.map_err(error)?;
            let metadata = remote.metadata().await.map_err(error)?;
            if !metadata.file_type().is_file() || metadata.len() > crate::files::ssh::LIMIT {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "Select a regular file within the transfer limit",
                ));
            }
            let mut local = tokio::fs::File::from_std(transfers.staging_file()?);
            let mut bytes = vec![0; sailry_protocol::FILE_TRANSFER_CHUNK_BYTES];
            let mut size = 0u64;
            let mut hash = blake3::Hasher::new();
            loop {
                let count = remote.read(&mut bytes).await.map_err(io_error)?;
                if count == 0 {
                    break;
                }
                size += count as u64;
                if size > crate::files::ssh::LIMIT {
                    return Err(Fault::new(
                        ErrorCode::InvalidRequest,
                        "SSH file exceeds the transfer limit",
                    ));
                }
                local.write_all(&bytes[..count]).await.map_err(io_error)?;
                hash.update(&bytes[..count]);
            }
            let after = remote.metadata().await.map_err(error)?;
            if size != metadata.len()
                || metadata.len() != after.len()
                || metadata.mtime != after.mtime
            {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "Remote file changed during download",
                ));
            }
            local.flush().await.map_err(io_error)?;
            local.rewind().await.map_err(io_error)?;
            transfers
                .download_ssh(
                    caller,
                    path,
                    local.into_std().await,
                    size,
                    hash.finalize().to_hex().to_string(),
                    closed,
                )
                .map(Outcome::Download)
        }
        Operation::Upload { path, stream } => {
            let source = transfers.take_ssh(caller, profile, &path, stream)?;
            if !source.spec.overwrite && sftp.symlink_metadata(path.clone()).await.is_ok() {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "Remote file already exists",
                ));
            }
            let flags = OpenFlags::WRITE
                | OpenFlags::CREATE
                | if source.spec.overwrite {
                    OpenFlags::TRUNCATE
                } else {
                    OpenFlags::EXCLUDE
                };
            let mut remote = sftp.open_with_flags(path, flags).await.map_err(error)?;
            let mut local = tokio::fs::File::from_std(source.file.try_clone().map_err(io_error)?);
            local.rewind().await.map_err(io_error)?;
            let bytes = tokio::io::copy(&mut local, &mut remote)
                .await
                .map_err(|_| unknown("SSH upload interrupted; inspect the remote file"))?;
            remote.shutdown().await.map_err(|_| {
                unknown("SSH upload completion is unavailable; inspect the remote file")
            })?;
            if bytes != source.spec.size {
                return Err(unknown("SSH upload size differs; inspect the remote file"));
            }
            Ok(Outcome::Transferred { bytes })
        }
    };
    let _ = sftp.close().await;
    outcome
}

fn error(error: russh_sftp::client::error::Error) -> Fault {
    use russh_sftp::{client::error::Error, protocol::StatusCode};
    let code = match &error {
        Error::Status(status) if status.status_code == StatusCode::PermissionDenied => {
            ErrorCode::PermissionDenied
        }
        Error::Status(status) if status.status_code == StatusCode::NoSuchFile => {
            ErrorCode::NotFound
        }
        _ => ErrorCode::Unavailable,
    };
    Fault::new(code, format!("SFTP operation failed: {error}"))
}
