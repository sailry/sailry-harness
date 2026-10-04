//! SCP framing adapted from sailry-code 67ae9fa0 sailry-connections/src/ssh.rs
//! (Apache-2.0). File bytes stay on the execution Node, outside control frames.
use super::*;
use crate::files::{
    io_error,
    ssh::{Incoming, LIMIT},
};
use russh::{Channel, client::Msg};
use sailry_protocol::ssh::{Direction, Transfer};
use std::{collections::VecDeque, path::PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) fn validate(transfer: &Transfer) -> Result<(), Fault> {
    crate::files::path::entry_components(&transfer.path)?;
    let remote = &transfer.remote_path;
    if remote.is_empty() || remote.len() > 4096 || remote.chars().any(char::is_control) {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid SSH remote path",
        ));
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
        let mut channel = session
            .channel_open_session()
            .await
            .map_err(|_| unavailable("SSH transfer channel could not be opened"))?;
        let mode = match transfer.direction {
            Direction::Upload => "-t",
            Direction::Download => "-f",
        };
        let quoted = format!("'{}'", transfer.remote_path.replace('\'', "'\\''"));
        channel
            .exec(true, format!("scp {mode} -- {quoted}"))
            .await
            .map_err(|_| interrupted())?;
        match &prepared {
            Prepared::Upload(file) => upload(&mut channel, file, &transfer.path).await,
            Prepared::Download(incoming) => download(&mut channel, &incoming.file).await,
        }
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
    channel: &mut Channel<Msg>,
    source: &std::fs::File,
    path: &str,
) -> Result<(u64, String), Fault> {
    let before = source.metadata().map_err(io_error)?;
    if before.len() > LIMIT {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "SSH file exceeds the transfer limit",
        ));
    }
    let mut file = tokio::fs::File::from_std(source.try_clone().map_err(io_error)?);
    let mut response = Response::default();
    response.ack(channel).await?;
    let name = path.rsplit('/').next().unwrap();
    channel
        .data_bytes(format!("C0600 {} {name}\n", before.len()).into_bytes())
        .await
        .map_err(|_| interrupted())?;
    response.ack(channel).await?;
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
        channel
            .data_bytes(buffer[..count].to_vec())
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
    channel
        .data_bytes(vec![0])
        .await
        .map_err(|_| interrupted())?;
    response.ack(channel).await?;
    channel.eof().await.map_err(|_| interrupted())?;
    response.finish(channel).await?;
    Ok((before.len(), String::new()))
}

async fn download(
    channel: &mut Channel<Msg>,
    destination: &std::fs::File,
) -> Result<(u64, String), Fault> {
    let mut response = Response::default();
    channel
        .data_bytes(vec![0])
        .await
        .map_err(|_| interrupted())?;
    let header = response.line(channel).await?;
    let size = file_header(&header)?;
    let mut file = tokio::fs::File::from_std(destination.try_clone().map_err(io_error)?);
    channel
        .data_bytes(vec![0])
        .await
        .map_err(|_| interrupted())?;
    let mut remaining = size;
    let mut hash = blake3::Hasher::new();
    while remaining > 0 {
        response.fill(channel).await?;
        let count = remaining.min(response.buffer.len() as u64) as usize;
        let bytes: Vec<_> = response.buffer.drain(..count).collect();
        file.write_all(&bytes).await.map_err(io_error)?;
        hash.update(&bytes);
        remaining -= count as u64;
    }
    file.flush().await.map_err(io_error)?;
    response.ack(channel).await?;
    channel
        .data_bytes(vec![0])
        .await
        .map_err(|_| interrupted())?;
    channel.eof().await.map_err(|_| interrupted())?;
    response.finish(channel).await?;
    Ok((size, hash.finalize().to_hex().to_string()))
}

fn file_header(header: &str) -> Result<u64, Fault> {
    let mut parts = header
        .strip_prefix('C')
        .ok_or_else(interrupted)?
        .splitn(3, ' ');
    let mode = parts.next().unwrap_or_default();
    let size = parts
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(interrupted)?;
    let name = parts.next().unwrap_or_default();
    if mode.len() != 4
        || !mode.bytes().all(|byte| matches!(byte, b'0'..=b'7'))
        || size > LIMIT
        || name.is_empty()
        || matches!(name, "." | "..")
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err(unknown(
            "SSH server returned an invalid or oversized SCP file",
        ));
    }
    Ok(size)
}

#[derive(Default)]
struct Response {
    buffer: VecDeque<u8>,
    exit: Option<u32>,
}
impl Response {
    async fn fill(&mut self, channel: &mut Channel<Msg>) -> Result<(), Fault> {
        while self.buffer.is_empty() {
            if self.exit.is_some() {
                return Err(interrupted());
            }
            let message = channel.wait().await.ok_or_else(interrupted)?;
            self.accept(message)?;
        }
        Ok(())
    }
    async fn byte(&mut self, channel: &mut Channel<Msg>) -> Result<u8, Fault> {
        self.fill(channel).await?;
        Ok(self.buffer.pop_front().unwrap())
    }
    async fn line(&mut self, channel: &mut Channel<Msg>) -> Result<String, Fault> {
        let mut line = Vec::new();
        loop {
            let byte = self.byte(channel).await?;
            if byte == b'\n' {
                return String::from_utf8(line).map_err(|_| interrupted());
            }
            if line.len() >= 4096 {
                return Err(interrupted());
            }
            line.push(byte);
        }
    }
    async fn ack(&mut self, channel: &mut Channel<Msg>) -> Result<(), Fault> {
        if self.byte(channel).await? == 0 {
            Ok(())
        } else {
            Err(interrupted())
        }
    }
    async fn finish(&mut self, channel: &mut Channel<Msg>) -> Result<(), Fault> {
        while let Some(message) = channel.wait().await {
            self.accept(message)?;
        }
        if self.exit == Some(0) && self.buffer.is_empty() {
            Ok(())
        } else {
            Err(interrupted())
        }
    }
    fn accept(&mut self, message: ChannelMsg) -> Result<(), Fault> {
        match message {
            ChannelMsg::Data { data } => {
                if self.buffer.len() + data.len() > 256 * 1024 {
                    return Err(interrupted());
                }
                self.buffer.extend(data);
            }
            ChannelMsg::ExitStatus { exit_status } => self.exit = Some(exit_status),
            ChannelMsg::Failure => return Err(interrupted()),
            _ => {}
        }
        Ok(())
    }
}
fn interrupted() -> Fault {
    unknown("SSH file transfer could not be confirmed; inspect the destination before retrying")
}
