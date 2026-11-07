//! Recover bounded pre-authentication TCP failures during macOS network transitions.
use std::{future::Future, io, time::Duration};
use tokio::net::TcpStream;

const RETRY_DELAYS: [Duration; 4] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
];

pub(super) async fn open(host: &str, port: u16) -> io::Result<TcpStream> {
    attempt(|| TcpStream::connect((host, port))).await
}

async fn attempt<Stream, Dial, Pending>(mut dial: Dial) -> io::Result<Stream>
where
    Dial: FnMut() -> Pending,
    Pending: Future<Output = io::Result<Stream>>,
{
    let mut result = dial().await;
    for delay in RETRY_DELAYS {
        if !matches!(&result, Err(error) if cfg!(target_os = "macos") && error.kind() == io::ErrorKind::HostUnreachable)
        {
            return result;
        }
        // Verified macOS local-network cache rebuilding can reject several TCP
        // attempts even with access allowed. TN3179 recommends connection recovery
        // for socket APIs: https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy
        // This bounded backoff runs before SSH handshake or authentication. The
        // caller retains its original deadline and cancellation; no remote operation
        // is replayed.
        tokio::time::sleep(delay).await;
        result = dial().await;
    }
    result
}

#[cfg(test)]
mod tests;
