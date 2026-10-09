use super::*;
use std::{io::Read, sync::Arc};
use tokio::{io::AsyncReadExt, sync::Notify};

async fn source(
    parts: Vec<Vec<u8>>,
    delay: Duration,
    stall: bool,
) -> (Network, tokio::task::JoinHandle<()>, Arc<Notify>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let network = Network::default();
    network.fixture(&format!("http://{}", listener.local_addr().unwrap()));
    let started = Arc::new(Notify::new());
    let ready = started.clone();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            header.push(byte[0]);
            assert!(header.len() < 8192);
        }
        let size = parts.iter().map(Vec::len).sum::<usize>() + usize::from(stall);
        stream
            .write_all(
                format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        for part in parts {
            stream.write_all(&part).await.unwrap();
            stream.flush().await.unwrap();
            ready.notify_one();
            tokio::time::sleep(delay).await;
        }
        if stall {
            std::future::pending::<()>().await;
        }
    });
    (network, task, started)
}

#[tokio::test]
async fn progress_resets_read_deadline() {
    let parts = vec![vec![b'a'; 16]; 6];
    let expected = parts.concat();
    let (network, source, _) = source(parts, Duration::from_millis(80), false).await;
    let started = std::time::Instant::now();
    let (mut file, digest) = network
        .download(
            "https://example.invalid/asset",
            1024,
            &CancellationToken::new(),
            Duration::from_millis(200),
        )
        .await
        .unwrap();
    assert!(started.elapsed() > Duration::from_millis(200));
    use std::io::Seek;
    file.rewind().unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, expected);
    assert_eq!(digest, format!("{:x}", Sha256::digest(&expected)));
    source.await.unwrap();
}

#[tokio::test]
async fn stalled_body_times_out() {
    let (network, source, _) = source(vec![b"partial".to_vec()], Duration::ZERO, true).await;
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        network.download(
            "https://example.invalid/asset",
            1024,
            &CancellationToken::new(),
            Duration::from_millis(200),
        ),
    )
    .await
    .expect("a stalled download must finish");
    assert_eq!(result.unwrap_err().code, ErrorCode::Unavailable);
    source.abort();
}

#[tokio::test]
async fn cancellation_interrupts_read() {
    let (network, source, started) = source(vec![b"partial".to_vec()], Duration::ZERO, true).await;
    let stop = CancellationToken::new();
    let cancelled = stop.clone();
    let download = tokio::spawn(async move {
        network
            .download(
                "https://example.invalid/asset",
                1024,
                &cancelled,
                Duration::from_secs(60),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), started.notified())
        .await
        .unwrap();
    stop.cancel();
    let result = tokio::time::timeout(Duration::from_secs(2), download)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    source.abort();
}
