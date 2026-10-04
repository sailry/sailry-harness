//! Byte and lifetime adapters only; the shared Client owns stream verification.
use super::*;
use sailry_client::Client;
use sailry_protocol::{FILE_TRANSFER_CHUNK_BYTES, Fault};
use std::{future::Future, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream},
    sync::watch,
};

pub(crate) mod download;
pub(crate) mod upload;
pub use download::Download;
pub use upload::Upload;

/// Hash a source in chunks before requesting an attachment upload descriptor.
#[frb(opaque)]
pub struct Digest {
    hasher: Mutex<blake3::Hasher>,
}

impl Digest {
    pub fn new() -> Self {
        Self {
            hasher: Mutex::new(blake3::Hasher::new()),
        }
    }

    /// Await each update in source order. Each chunk may contain at most 64 KiB.
    pub async fn update(&self, bytes: Vec<u8>) -> Result<(), String> {
        check_chunk(&bytes)?;
        self.hasher.lock().await.update(&bytes);
        Ok(())
    }

    pub async fn revision(&self) -> String {
        self.hasher.lock().await.finalize().to_hex().to_string()
    }
}

impl Default for Digest {
    fn default() -> Self {
        Self::new()
    }
}

struct Transfer {
    stop: CancellationToken,
    result: watch::Receiver<Option<Result<(), String>>>,
}

impl Transfer {
    fn spawn(
        stop: CancellationToken,
        future: impl Future<Output = Result<(), Fault>> + Send + 'static,
    ) -> Self {
        let (sender, result) = watch::channel(None);
        // Cancellation lets the Client release the Node resource. Aborting the
        // task would skip that cleanup; it never retains the owning FFI handle.
        tokio::spawn(async move {
            sender.send_replace(Some(future.await.map_err(error)));
        });
        Self { stop, result }
    }

    async fn result(&self) -> Result<(), String> {
        let mut receiver = self.result.clone();
        loop {
            if let Some(result) = receiver.borrow_and_update().clone() {
                return result;
            }
            tokio::select! {
                biased;
                _ = self.stop.cancelled() => return Err(closed()),
                result = receiver.changed() => result.map_err(|_| "transfer task ended".to_string())?,
            }
        }
    }
}

impl Drop for Transfer {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

fn check_chunk(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > FILE_TRANSFER_CHUNK_BYTES {
        Err("transfer chunk exceeds 64 KiB".into())
    } else {
        Ok(())
    }
}

fn closed() -> String {
    "transfer is closed".into()
}

#[cfg(test)]
mod tests;
