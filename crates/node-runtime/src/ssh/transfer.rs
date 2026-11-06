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
    .map_err(|_| {
        at_stage(
            unavailable("SSH file preparation failed"),
            "local preparation",
        )
    })?
    .map_err(|error| at_stage(error, "local preparation"))?;
    let work = async {
        let sftp = files::open(session)
            .await
            .map_err(|error| at_stage(error, "remote subsystem"))?;
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
        _ = stop.cancelled() => return Err(interrupted("outer cancelled")),
        _ = closed.cancelled() => return Err(interrupted("outer closed")),
        result = tokio::time::timeout(Duration::from_millis(timeout_ms), work) => result.map_err(|_| interrupted("outer timeout"))??,
    };
    if let Prepared::Download(incoming) = prepared {
        if stop.is_cancelled() {
            return Err(interrupted("outer cancelled"));
        }
        if closed.is_cancelled() {
            return Err(interrupted("outer closed"));
        }
        // Once publication starts, retain and await the existing save worker through
        // shutdown. Dropping an async future cannot cancel a filesystem write.
        tokio::task::spawn_blocking(move || incoming.publish(bytes, &revision))
            .await
            .map_err(|_| interrupted("local publish worker"))?
            .map_err(|error| at_stage(error, "local publish"))?;
    }
    Ok(Outcome::Transferred { bytes })
}

async fn upload(
    sftp: &SftpSession,
    source: &std::fs::File,
    path: &str,
    remote_path: &str,
) -> Result<(u64, String), Fault> {
    let before = source
        .metadata()
        .map_err(|error| local_failure("local metadata", error))?;
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
        Err(error) => return Err(sftp_failure("remote stat", error)),
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
        .map_err(|error| sftp_failure("remote open", error))?;
    let mut file = tokio::fs::File::from_std(
        source
            .try_clone()
            .map_err(|error| local_failure("local clone", error))?,
    );
    let mut remaining = before.len();
    let mut buffer = vec![0; sailry_protocol::FILE_TRANSFER_CHUNK_BYTES];
    while remaining > 0 {
        let limit = remaining.min(buffer.len() as u64) as usize;
        let count = file
            .read(&mut buffer[..limit])
            .await
            .map_err(|error| io_failure("local read", error))?;
        if count == 0 {
            return Err(interrupted("source EOF"));
        }
        remote
            .write_all(&buffer[..count])
            .await
            .map_err(|error| io_failure("remote write", error))?;
        remaining -= count as u64;
    }
    let after = file
        .metadata()
        .await
        .map_err(|error| io_failure("local metadata", error))?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err(at_stage(
            unknown("SSH upload source changed; inspect the remote file"),
            "local metadata",
        ));
    }
    remote
        .shutdown()
        .await
        .map_err(|error| io_failure("remote close", error))?;
    Ok((before.len(), String::new()))
}

async fn download(
    sftp: &SftpSession,
    destination: &std::fs::File,
    path: &str,
) -> Result<(u64, String), Fault> {
    let mut remote = sftp
        .open(path)
        .await
        .map_err(|error| sftp_failure("remote open", error))?;
    let before = remote
        .metadata()
        .await
        .map_err(|error| sftp_failure("remote stat", error))?;
    if !before.file_type().is_file() || before.len() > LIMIT {
        return Err(at_stage(
            unknown("SSH server returned an invalid or oversized file"),
            "remote stat",
        ));
    }
    let mut file = tokio::fs::File::from_std(
        destination
            .try_clone()
            .map_err(|error| local_failure("local clone", error))?,
    );
    let mut buffer = vec![0; sailry_protocol::FILE_TRANSFER_CHUNK_BYTES];
    let mut bytes = 0u64;
    let mut hash = blake3::Hasher::new();
    loop {
        let count = remote
            .read(&mut buffer)
            .await
            .map_err(|error| io_failure("remote read", error))?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > LIMIT {
            return Err(interrupted("remote read limit"));
        }
        file.write_all(&buffer[..count])
            .await
            .map_err(|error| local_failure("local write", error))?;
        hash.update(&buffer[..count]);
    }
    let after = remote
        .metadata()
        .await
        .map_err(|error| sftp_failure("remote stat", error))?;
    if bytes != before.len() || before.len() != after.len() || before.mtime != after.mtime {
        return Err(at_stage(
            unknown("SSH download source changed; inspect the remote file"),
            "remote stat",
        ));
    }
    file.flush()
        .await
        .map_err(|error| local_failure("local flush", error))?;
    remote
        .shutdown()
        .await
        .map_err(|error| io_failure("remote close", error))?;
    Ok((bytes, hash.finalize().to_hex().to_string()))
}

fn interrupted(stage: &str) -> Fault {
    at_stage(
        unknown(
            "SSH file transfer could not be confirmed; inspect the destination before retrying",
        ),
        stage,
    )
}

fn sftp_failure(stage: &str, error: Error) -> Fault {
    let cause = match error {
        Error::Status(status) => format!(
            "SFTP status={:?} ({}): {}",
            status.status_code,
            status.status_code as u32,
            bounded(&status.error_message)
        ),
        Error::IO(message) => format!("SFTP IO: {}", bounded(&message)),
        Error::Timeout => "SFTP Timeout".into(),
        Error::Limited(message) => format!("SFTP Limited: {}", bounded(&message)),
        Error::UnexpectedPacket => "SFTP UnexpectedPacket".into(),
        Error::UnexpectedBehavior(message) => {
            format!("SFTP UnexpectedBehavior: {}", bounded(&message))
        }
    };
    interrupted(&format!("{stage}: {cause}"))
}

fn io_failure(stage: &str, error: std::io::Error) -> Fault {
    interrupted(&format!(
        "{stage}: I/O kind={:?}, os_error={:?}: {}",
        error.kind(),
        error.raw_os_error(),
        bounded(&error.to_string())
    ))
}

fn local_failure(stage: &str, error: std::io::Error) -> Fault {
    let stage = format!(
        "{stage}: I/O kind={:?}, os_error={:?}",
        error.kind(),
        error.raw_os_error()
    );
    at_stage(io_error(error), &stage)
}

fn at_stage(mut error: Fault, stage: &str) -> Fault {
    error.message = format!("{} ({stage})", bounded(&error.message));
    error
}

fn bounded(message: &str) -> String {
    message
        .chars()
        .filter(|character| !character.is_control())
        .take(256)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh_sftp::protocol::{Attrs, Status};

    #[tokio::test]
    async fn distinguishes_remote_stages() {
        struct Peer {
            deny_stat: bool,
        }
        impl russh_sftp::server::Handler for Peer {
            type Error = StatusCode;

            fn unimplemented(&self) -> Self::Error {
                StatusCode::PermissionDenied
            }

            async fn stat(&mut self, _: u32, _: String) -> Result<Attrs, Self::Error> {
                Err(if self.deny_stat {
                    StatusCode::PermissionDenied
                } else {
                    StatusCode::NoSuchFile
                })
            }
        }

        for (deny_stat, stage) in [(true, "remote stat"), (false, "remote open")] {
            let (stream, peer) = tokio::io::duplex(4096);
            russh_sftp::server::run(peer, Peer { deny_stat }).await;
            let sftp = SftpSession::new(stream).await.unwrap();
            let source = tempfile::tempfile().unwrap();
            let error = upload(&sftp, &source, "source.bin", "destination.bin")
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::OutcomeUnknown);
            assert!(error.message.contains(&format!(
                "{stage}: SFTP status=PermissionDenied (3): Permission denied"
            )));
            sftp.close().await.unwrap();
        }
    }

    #[test]
    fn classifies_sftp_errors() {
        for (error, cause) in [
            (Error::IO("disconnected".into()), "SFTP IO: disconnected"),
            (Error::Timeout, "SFTP Timeout"),
            (
                Error::Limited("packet size".into()),
                "SFTP Limited: packet size",
            ),
            (Error::UnexpectedPacket, "SFTP UnexpectedPacket"),
            (
                Error::UnexpectedBehavior("closed response".into()),
                "SFTP UnexpectedBehavior: closed response",
            ),
        ] {
            let error = sftp_failure("remote stat", error);
            assert_eq!(error.code, ErrorCode::OutcomeUnknown);
            assert!(error.message.ends_with(&format!("(remote stat: {cause})")));
        }
    }

    #[test]
    fn preserves_io_classification() {
        let error = io_failure("remote close", std::io::Error::other("close denied"));
        assert_eq!(error.code, ErrorCode::OutcomeUnknown);
        assert!(
            error
                .message
                .ends_with("(remote close: I/O kind=Other, os_error=None: close denied)")
        );
        assert!(!error.message.contains("SFTP status="));
        let error = io_failure("remote write", std::io::Error::from_raw_os_error(5));
        assert_eq!(error.code, ErrorCode::OutcomeUnknown);
        assert!(error.message.contains("remote write: I/O kind="));
        assert!(error.message.contains("os_error=Some(5)"));
        let error = local_failure(
            "local metadata",
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "metadata denied"),
        );
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert!(
            error
                .message
                .contains("local metadata: I/O kind=PermissionDenied")
        );
    }

    #[test]
    fn bounds_untrusted_messages() {
        let message = format!("\0{}\r\n", "界".repeat(512));
        for error in [
            sftp_failure(
                "remote stat",
                Error::Status(Status {
                    id: 17,
                    status_code: StatusCode::Failure,
                    error_message: message.clone(),
                    language_tag: "ignored-language-tag".into(),
                }),
            ),
            sftp_failure("remote open", Error::IO(message.clone())),
            sftp_failure("remote open", Error::Limited(message.clone())),
            sftp_failure("remote open", Error::UnexpectedBehavior(message.clone())),
            io_failure("remote close", std::io::Error::other(message.clone())),
            local_failure("local read", std::io::Error::other(message)),
        ] {
            assert!(error.message.chars().count() <= 512);
            assert!(!error.message.chars().any(char::is_control));
            assert!((1..=256).contains(&error.message.matches('界').count()));
            assert!(!error.message.contains("ignored-language-tag"));
        }
    }
}
