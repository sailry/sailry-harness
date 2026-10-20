use super::*;
use adk_core::{LlmRequest, LlmResponseStream};
use async_trait::async_trait;
struct Fixture;
#[async_trait]
impl Llm for Fixture {
    fn name(&self) -> &str {
        "fixture"
    }
    async fn generate_content(
        &self,
        _: LlmRequest,
        _: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        Ok(Box::pin(futures::stream::empty()))
    }
}
#[test]
fn reuses_and_evicts() {
    let registry = Registry::default();
    let session = SessionId::new();
    let first = registry
        .get(session, "one".into(), || Ok(Built::new(Arc::new(Fixture))))
        .unwrap();
    let second = registry
        .get(session, "one".into(), || panic!("client must be reused"))
        .unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    let changed = registry
        .get(session, "two".into(), || Ok(Built::new(Arc::new(Fixture))))
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &changed));
    registry.evict(session);
    assert!(registry.entries.lock().unwrap().is_empty());
}
#[test]
fn expires_and_bounds_entries() {
    let registry = Registry::default();
    let session = SessionId::new();
    registry
        .get(session, "one".into(), || Ok(Built::new(Arc::new(Fixture))))
        .unwrap();
    registry
        .entries
        .lock()
        .unwrap()
        .values_mut()
        .next()
        .unwrap()
        .used -= IDLE;
    registry
        .get(session, "one".into(), || Ok(Built::new(Arc::new(Fixture))))
        .unwrap();
    for _ in 0..CAPACITY + 5 {
        registry
            .get(SessionId::new(), "one".into(), || {
                Ok(Built::new(Arc::new(Fixture)))
            })
            .unwrap();
    }
    assert_eq!(registry.entries.lock().unwrap().len(), CAPACITY);
    registry.clear();
    assert!(registry.entries.lock().unwrap().is_empty());
}

#[tokio::test]
async fn reuses_http_connections() {
    use adk_core::Content;
    use futures::StreamExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let connections = Arc::new(AtomicUsize::new(0));
    let count = connections.clone();
    let server = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        while let Ok((mut socket, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            tasks.spawn(async move {
                loop {
                    let mut data = Vec::new();
                    let mut buffer = [0; 4096];
                    let end = loop {
                        let read = socket.read(&mut buffer).await.unwrap();
                        if read == 0 { return; }
                        data.extend_from_slice(&buffer[..read]);
                        if let Some(end) = data.windows(4).position(|value| value == b"\r\n\r\n") { break end + 4; }
                    };
                    let length = std::str::from_utf8(&data[..end]).unwrap().lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().unwrap())
                    }).unwrap();
                    while data.len() < end + length {
                        let read = socket.read(&mut buffer).await.unwrap(); if read == 0 { return; } data.extend_from_slice(&buffer[..read]);
                    }
                    let body = serde_json::json!({"id":"resp","object":"response","created_at":0,"model":"fixture","status":"completed","output":[{"type":"message","id":"msg","role":"assistant","status":"completed","content":[{"type":"output_text","text":"answer","annotations":[]}]}]}).to_string();
                    let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",body.len());
                    if socket.write_all(reply.as_bytes()).await.is_err() { return; }
                }
            });
        }
    });
    let registry = Registry::default();
    let session = SessionId::new();
    for _ in 0..2 {
        let entry = registry
            .get(session, "same".into(), || {
                Ok(Built::new(Arc::new(
                    adk_model::openai::OpenAIResponsesClient::new(
                        adk_model::openai::OpenAIResponsesConfig::new("fixture", "fixture")
                            .with_base_url(&endpoint)
                            .with_open_responses_mode(true),
                    )
                    .unwrap(),
                )))
            })
            .unwrap();
        let mut stream = entry
            .inner
            .generate_content(
                LlmRequest::new("fixture", vec![Content::new("user").with_text("request")]),
                false,
            )
            .await
            .unwrap();
        assert!(stream.next().await.unwrap().unwrap().turn_complete);
    }
    assert_eq!(connections.load(Ordering::SeqCst), 1);
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn owns_unused_preconnections() {
    use futures::StreamExt;
    let _ = rustls::crypto::ring::default_provider().install_default();
    for release in 0..4 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let (opened, handshake) = tokio::sync::oneshot::channel();
        let (closed, disconnected) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let _ = opened.send(());
            let message = socket.next().await;
            assert!(!matches!(
                message,
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(_)))
            ));
            let _ = closed.send(());
        });
        let client = Arc::new(
            OpenAIResponsesClient::new(
                adk_model::openai::OpenAIResponsesConfig::new("fixture", "fixture")
                    .with_base_url(endpoint),
            )
            .unwrap()
            .with_websocket(Default::default()),
        );
        let socket = client.websocket().unwrap();
        let registry = Registry::default();
        let session = SessionId::new();
        let model = client.clone();
        let entry = registry
            .get(session, "socket".into(), || {
                Ok(Built {
                    inner: model.clone(),
                    responses: Some(model),
                })
            })
            .unwrap();
        let stop = CancellationToken::new();
        entry.preconnect(&stop, async move {
            let _ = client.preconnect().await;
        });
        tokio::time::timeout(Duration::from_secs(2), handshake)
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !socket.is_connected() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        tokio::task::yield_now().await;
        match release {
            0 => stop.cancel(),
            1 => registry.evict(session),
            2 => registry.clear(),
            _ => {
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(26)).await;
                tokio::task::yield_now().await;
                tokio::time::resume();
            }
        }
        tokio::time::timeout(Duration::from_secs(2), disconnected)
            .await
            .unwrap()
            .unwrap();
        assert!(!socket.is_connected());
        server.await.unwrap();
    }
}
