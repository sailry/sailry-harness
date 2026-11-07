use super::*;
use futures::StreamExt;

#[tokio::test]
async fn closes_an_active_http_stream() {
    let prefix = fixtures::sse([
        json!({
            "type":"response.created","sequence_number":0,
            "response":{"id":"pending","object":"response","created_at":1,
                "model":"fixture","status":"in_progress","output":[]}
        }),
        json!({
            "type":"response.output_text.delta","sequence_number":1,"item_id":"message",
            "output_index":0,"content_index":0,"delta":"Partial","logprobs":[]
        }),
    ]);
    let server = stalled::Server::start(prefix).await;
    let provider = provider(ModelApi::Responses, server.endpoint.clone());
    let model = native(&provider, "fixture", "key", HeaderMap::new());
    let cancel = CancellationToken::new();
    let mut stream = model
        .call(request("fixture"), cancel.clone())
        .await
        .unwrap();
    let first = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = stream
                .next()
                .await
                .expect("stream ended before its first text delta")
                .unwrap();
            if let Event::TextDelta { text, .. } = event {
                break text;
            }
        }
    })
    .await
    .expect("initial stream delta");
    assert_eq!(first, "Partial");
    server.ready().await;
    cancel.cancel();
    let remainder = tokio::time::timeout(Duration::from_secs(5), async {
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }
        events
    })
    .await
    .expect("cancelled stream deadline");
    assert!(
        remainder
            .iter()
            .any(|event| matches!(event, Err(error) if error.kind == ErrorKind::Cancelled))
    );
    assert!(
        remainder
            .iter()
            .all(|event| !matches!(event, Ok(Event::Finish(_))))
    );
    server.closed().await;
}

#[tokio::test]
async fn dropping_the_stream_closes_http() {
    let prefix = fixtures::sse([json!({
        "id":"chat","object":"chat.completion.chunk","created":1,"model":"fixture",
        "choices":[{"index":0,"delta":{"content":"Partial"},"finish_reason":null}]
    })]);
    let server = stalled::Server::start(prefix).await;
    let provider = provider(ModelApi::ChatCompletions, server.endpoint.clone());
    let model = native(&provider, "fixture", "key", HeaderMap::new());
    let mut stream = model
        .call(request("fixture"), CancellationToken::new())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    server.ready().await;
    drop(stream);
    server.closed().await;
}
