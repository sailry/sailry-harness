//! Recover a single pre-authentication TCP failure during macOS network transitions.
use std::{future::Future, io, time::Duration};
use tokio::net::TcpStream;

const RETRY_DELAY: Duration = Duration::from_millis(250);

pub(super) async fn open(host: &str, port: u16) -> io::Result<TcpStream> {
    attempt(|| TcpStream::connect((host, port))).await
}

async fn attempt<Stream, Dial, Pending>(mut dial: Dial) -> io::Result<Stream>
where
    Dial: FnMut() -> Pending,
    Pending: Future<Output = io::Result<Stream>>,
{
    match dial().await {
        Err(error)
            if cfg!(target_os = "macos") && error.kind() == io::ErrorKind::HostUnreachable =>
        {
            // Verified macOS local-network cache rebuilding can reject the first TCP
            // attempt even with access allowed. TN3179 recommends connection recovery
            // for socket APIs: https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy
            // This runs before SSH handshake or authentication. The caller retains its
            // original deadline and cancellation; no remote operation is replayed.
            tokio::time::sleep(RETRY_DELAY).await;
            dial().await
        }
        result => result,
    }
}

#[cfg(test)]
mod tests;
