//! Transfer handling adapted from sailry-code 67ae9fa0 sailry-connections/src/ssh.rs
//! (Apache-2.0). File bytes stay on the execution Node, outside control frames.
use super::*;
use crate::files::{
    io_error,
    ssh::{Incoming, LIMIT},
};
use russh_sftp::{
    client::{SftpSession, error::Error},
    protocol::{FileAttributes, OpenFlags, StatusCode},
};
use sailry_protocol::ssh::{Direction, Transfer};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) fn validate(transfer: &Transfer) -> Result<(), Fault> {
    crate::files::path::entry_components(&transfer.path)?;
    let remote = &transfer.remote_path;
    if remote.is_empty() || remote.len() > 4096 || remote.chars().any(char::is_control) {
        return Err(Fault::invalid("invalid SSH remote path"));
    }
    Ok(())
}

enum Prepared {
    Upload(std::fs::File),
    Download(Incoming),
}

pub(super) async fn execute(
    session: &client::Handle<Verifier>,
    root: PathBuf,
    profile: Option<PathBuf>,
    transfer: Transfer,
    timeout_ms: u64,
    stop: &CancellationToken,
    closed: &CancellationToken,
) -> Result<Outcome, Fault> {
    let spec = transfer.clone();
    let prepared = tokio::task::spawn_blocking(move || match spec.direction {
        Direction::Upload => {
            crate::files::ssh::source(&root, &spec.path, profile.as_deref()).map(Prepared::Upload)
        }
        Direction::Download => {
            Incoming::prepare(&root, &spec.path, profile.as_deref()).map(Prepared::Download)
        }
    })
    .await
    .map_err(|_| unavailable("SSH file preparation failed"))??;
    let work = async {
        let sftp = files::open(session).await?;
        let result = match &prepared {
            Prepared::Upload(file) => {
                upload(&sftp, file, &transfer.path, &transfer.remote_path).await
            }
            Prepared::Download(incoming) => {
                download(&sftp, &incoming.file, &transfer.remote_path).await
            }
        };
        let _ = sftp.close().await;
        result
    };
    let (bytes, revision) = tokio::select! {
        biased;
        _ = stop.cancelled() => return Err(interrupted()),
        _ = closed.cancelled() => return Err(interrupted()),
        result = tokio::time::timeout(Duration::from_millis(timeout_ms), work) => result.map_err(|_| interrupted())??,
    };
    if let Prepared::Download(incoming) = prepared {
        if stop.is_cancelled() || closed.is_cancelled() {
            return Err(interrupted());
        }
        // Once publication starts, retain and await the existing save worker through
        // shutdown. Dropping an async future cannot cancel a filesystem write.
        tokio::task::spawn_blocking(move || incoming.publish(bytes, &revision))
            .await
            .map_err(|_| interrupted())??;
    }
    Ok(Outcome::Transferred { bytes })
}

async fn upload(
    sftp: &SftpSession,
    source: &std::fs::File,
    path: &str,
    remote_path: &str,
) -> Result<(u64, String), Fault> {
    let before = source.metadata().map_err(io_error)?;
    if before.len() > LIMIT {
        return Err(Fault::invalid("SSH file exceeds the transfer limit"));
    }
    let remote_path = match sftp.metadata(remote_path).await {
        Ok(metadata) if metadata.is_dir() => format!(
            "{}/{}",
            remote_path.trim_end_matches('/'),
            path.rsplit('/').next().unwrap()
        ),
        Ok(_) => remote_path.to_owned(),
        Err(Error::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
            remote_path.to_owned()
        }
        Err(_) => return Err(interrupted()),
    };
    let mut remote = sftp
        .open_with_flags_and_attributes(
            remote_path,
            OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
            FileAttributes {
                permissions: Some(0o600),
                ..FileAttributes::empty()
            },
        )
        .await
        .map_err(|_| interrupted())?;
    let mut file = tokio::fs::File::from_std(source.try_clone().map_err(io_error)?);
    let mut remaining = before.len();
    let mut buffer = vec![0; sailry_protocol::FILE_TRANSFER_CHUNK_BYTES];
    while remaining > 0 {
        let limit = remaining.min(buffer.len() as u64) as usize;
        let count = file
            .read(&mut buffer[..limit])
            .await
            .map_err(|_| interrupted())?;
        if count == 0 {
            return Err(interrupted());
        }
        remote
            .write_all(&buffer[..count])
            .await
            .map_err(|_| interrupted())?;
        remaining -= count as u64;
    }
    let after = file.metadata().await.map_err(|_| interrupted())?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err(unknown(
            "SSH upload source changed; inspect the remote file",
        ));
    }
    remote.shutdown().await.map_err(|_| interrupted())?;
    Ok((before.len(), String::new()))
}

async fn download(
    sftp: &SftpSession,
    destination: &std::fs::File,
    path: &str,
) -> Result<(u64, String), Fault> {
    let mut remote = sftp.open(path).await.map_err(|_| interrupted())?;
    let before = remote.metadata().await.map_err(|_| interrupted())?;
    if !before.file_type().is_file() || before.len() > LIMIT {
        return Err(unknown("SSH server returned an invalid or oversized file"));
    }
    let mut file = tokio::fs::File::from_std(destination.try_clone().map_err(io_error)?);
    let mut buffer = vec![0; sailry_protocol::FILE_TRANSFER_CHUNK_BYTES];
    let mut bytes = 0u64;
    let mut hash = blake3::Hasher::new();
    loop {
        let count = remote.read(&mut buffer).await.map_err(|_| interrupted())?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > LIMIT {
            return Err(interrupted());
        }
        file.write_all(&buffer[..count]).await.map_err(io_error)?;
        hash.update(&buffer[..count]);
    }
    let after = remote.metadata().await.map_err(|_| interrupted())?;
    if bytes != before.len() || before.len() != after.len() || before.mtime != after.mtime {
        return Err(unknown(
            "SSH download source changed; inspect the remote file",
        ));
    }
    file.flush().await.map_err(io_error)?;
    remote.shutdown().await.map_err(|_| interrupted())?;
    Ok((bytes, hash.finalize().to_hex().to_string()))
}

fn interrupted() -> Fault {
    unknown("SSH file transfer could not be confirmed; inspect the destination before retrying")
}
