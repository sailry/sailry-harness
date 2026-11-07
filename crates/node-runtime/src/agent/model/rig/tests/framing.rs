use super::*;
use futures::StreamExt;

#[tokio::test]
async fn accepts_legacy_event_lines() {
    let body = fixtures::responses(false);
    let events = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .collect::<Vec<_>>();
    for (prefix, ending, terminal) in [
        ("", "\n", "\n"),
        ("data:", "\n", "\n"),
        ("", "\r\n", ""),
        ("data: ", "\r\n", ""),
    ] {
        let body = events
            .iter()
            .map(|event| format!("{prefix}{event}"))
            .collect::<Vec<_>>()
            .join(ending);
        let body = format!(
            ": heartbeat\nevent: response.created\nid: fixture\nretry: 50\nx-field: optional\n\n{body}{terminal}"
        );
        let server = Server::start(vec![body]).await;
        let provider = provider(ModelApi::Responses, server.endpoint.clone());
        let model = native(&provider, "fixture", "key", HeaderMap::new());
        let events = collect(&model, request("fixture"), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(text(&events), fixtures::ANSWER);
        assert_eq!(events.last(), Some(&Event::Finish(Finish::Stop)));
        assert!(events.iter().any(|event| {
            matches!(event, Event::Usage(usage)
                if usage.input == Some(12) && usage.output == Some(3) && usage.total == Some(15))
        }));
        assert_eq!(server.finish().await.len(), 1);
    }
}

#[tokio::test]
async fn rejects_unary_responses() {
    let body = fixtures::responses(false);
    let completed: Value = serde_json::from_str(
        body.lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .last()
            .unwrap(),
    )
    .unwrap();
    for prefix in ["", "data: "] {
        let server = Server::start(vec![format!("{prefix}{}", completed["response"])]).await;
        let provider = provider(ModelApi::Responses, server.endpoint.clone());
        let model = native(&provider, "fixture", "key", HeaderMap::new());
        let error = collect(&model, request("fixture"), CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidResponse);
        assert_eq!(server.finish().await.len(), 1);
    }
}

#[tokio::test]
async fn delivers_before_completion_and_cancels() {
    let body = fixtures::sse([
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
    for prefix in ["", "data: "] {
        let prefix = body
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|event| format!("{prefix}{event}\n"))
            .collect::<String>();
        let server = stalled::Server::start(prefix).await;
        let provider = provider(ModelApi::Responses, server.endpoint.clone());
        let model = native(&provider, "fixture", "key", HeaderMap::new());
        let cancel = CancellationToken::new();
        let mut stream = model
            .call(request("fixture"), cancel.clone())
            .await
            .unwrap();
        let text = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match stream.next().await.unwrap().unwrap() {
                    Event::TextDelta { text, .. } => break text,
                    Event::Finish(_) => panic!("response finished before its text"),
                    _ => {}
                }
            }
        })
        .await
        .expect("incremental event deadline");
        assert_eq!(text, "Partial");
        server.ready().await;
        cancel.cancel();
        let error = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(event) = stream.next().await {
                match event {
                    Ok(Event::Finish(_)) => panic!("cancelled response finished"),
                    Err(error) => return error,
                    _ => {}
                }
            }
            panic!("cancelled response succeeded")
        })
        .await
        .expect("cancellation deadline");
        assert_eq!(error.kind, ErrorKind::Cancelled);
        server.closed().await;
    }
}

#[tokio::test]
async fn preserves_transport_failure() {
    for prefix in ["", "data: "] {
        let delta = json!({
            "type":"response.output_text.delta","sequence_number":0,"item_id":"message",
            "output_index":0,"content_index":0,"delta":"Partial","logprobs":[]
        });
        let body = format!("{prefix}{delta}\n{prefix}{delta}");
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len() + 100
        );
        let server = Server::start(vec![reply]).await;
        let provider = provider(ModelApi::Responses, server.endpoint.clone());
        let model = native(&provider, "fixture", "key", HeaderMap::new());
        let mut stream = model
            .call(request("fixture"), CancellationToken::new())
            .await
            .unwrap();
        let mut text = String::new();
        let mut failure = None;
        while let Some(event) = stream.next().await {
            match event {
                Ok(Event::TextDelta { text: delta, .. }) => text.push_str(&delta),
                Ok(Event::Finish(_)) => panic!("broken HTTP response finished"),
                Err(error) => failure = Some(error),
                _ => {}
            }
        }
        assert_eq!(text, "Partial");
        assert_eq!(failure.unwrap().kind, ErrorKind::Network);
        assert_eq!(server.finish().await.len(), 1);
    }
}
