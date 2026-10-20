#![cfg(feature = "openai")]

use adk_core::{Content, Llm, LlmRequest, LlmResponse, Part};
use adk_model::openai::{
    OpenAIResponsesClient, OpenAIResponsesConfig, RequestAdapter, ResponseObserver,
};
use adk_model::retry::RetryConfig;
use futures::TryStreamExt;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

fn response() -> Value {
    json!({
        "id": "resp_fixture",
        "object": "response",
        "created_at": 0,
        "model": "fixture",
        "status": "completed",
        "output": [{
            "type": "message",
            "id": "msg_fixture",
            "role": "assistant",
            "status": "completed",
            "content": [{"type": "output_text", "text": "Hello", "annotations": []}]
        }],
        "usage": {
            "input_tokens": 10,
            "input_tokens_details": {"cached_tokens": 2},
            "output_tokens": 5,
            "output_tokens_details": {"reasoning_tokens": 1},
            "total_tokens": 15
        }
    })
}

fn client(base_url: &str, compatible: bool) -> OpenAIResponsesClient {
    OpenAIResponsesClient::new(
        OpenAIResponsesConfig::new("fixture", "fixture")
            .with_base_url(base_url)
            .with_open_responses_mode(compatible),
    )
    .expect("client should be created")
    .with_retry_config(RetryConfig::disabled())
}

async fn generate(
    client: &OpenAIResponsesClient,
    streaming: bool,
) -> Result<Vec<LlmResponse>, adk_core::AdkError> {
    let request = LlmRequest::new("fixture", vec![Content::new("user").with_text("Hello")]);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        client.generate_content(request, streaming).await?.try_collect().await
    })
    .await
    .expect("generation should terminate")
}

fn assert_completion(results: &[LlmResponse]) {
    let text: String = results
        .iter()
        .filter_map(|response| response.content.as_ref())
        .flat_map(|content| &content.parts)
        .filter_map(|part| match part {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello");
    assert_eq!(results.iter().filter(|result| result.turn_complete).count(), 1);
    assert_eq!(results.last().unwrap().usage_metadata.as_ref().unwrap().total_token_count, 15);
}

#[tokio::test]
async fn preserves_builders_in_both_orders() {
    for streaming in [false, true] {
        for compatible in [false, true] {
            for observer_first in [false, true] {
                let server = MockServer::start().await;
                let template = if streaming {
                    let delta = json!({
                        "type": "response.output_text.delta",
                        "sequence_number": 1,
                        "item_id": "msg_fixture",
                        "output_index": 0,
                        "content_index": 0,
                        "delta": "Hello"
                    });
                    let completed = json!({
                        "type": "response.completed",
                        "sequence_number": 2,
                        "response": response()
                    });
                    ResponseTemplate::new(200).set_body_raw(
                        format!("data: {delta}\n\ndata: {completed}\n\n"),
                        "text/event-stream",
                    )
                } else {
                    ResponseTemplate::new(200).set_body_json(response())
                };
                Mock::given(method("POST"))
                    .and(path("/responses"))
                    .and(header("x-fixture-default", "default"))
                    .and(header("x-fixture-request", "custom"))
                    .respond_with(template.insert_header("x-fixture-observed", "available"))
                    .expect(1)
                    .mount(&server)
                    .await;

                let calls = Arc::new(AtomicUsize::new(0));
                let observed = calls.clone();
                let observer: ResponseObserver = Arc::new(move |headers| {
                    assert_eq!(headers["x-fixture-observed"], "available");
                    observed.fetch_add(1, Ordering::SeqCst);
                });
                let adapter: RequestAdapter = Arc::new(|body, headers| {
                    body["metadata"] = json!({"fixture": "custom"});
                    headers.insert("x-fixture-request", "custom".parse().unwrap());
                    Ok(())
                });
                let mut headers = http::HeaderMap::new();
                headers.insert("x-fixture-default", "default".parse().unwrap());
                let client = client(&server.uri(), compatible);
                let client = if observer_first {
                    client
                        .with_response_observer(observer)
                        .with_request_adapter(adapter)
                        .with_default_headers(headers)
                        .expect("default headers should be accepted")
                } else {
                    client
                        .with_default_headers(headers)
                        .expect("default headers should be accepted")
                        .with_request_adapter(adapter)
                        .with_response_observer(observer)
                };

                assert_completion(
                    &generate(&client, streaming).await.expect("generation should succeed"),
                );
                assert_eq!(calls.load(Ordering::SeqCst), 1);
                let requests = server.received_requests().await.unwrap();
                assert_eq!(requests.len(), 1);
                let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
                assert_eq!(body["metadata"]["fixture"], "custom");
                assert_eq!(body.get("stream"), Some(&Value::Bool(streaming)));
            }
        }
    }
}

#[tokio::test]
async fn observes_unsuccessful_attempts() {
    for streaming in [false, true] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(
                ResponseTemplate::new(503)
                    .insert_header("x-fixture-observed", "failure")
                    .set_body_json(json!({
                        "error": {"message": "retry fixture", "type": "server_error"}
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let observer: ResponseObserver = Arc::new(move |headers| {
            assert_eq!(headers["x-fixture-observed"], "failure");
            observed.fetch_add(1, Ordering::SeqCst);
        });
        let client = client(&server.uri(), true).with_response_observer(observer);
        assert!(generate(&client, streaming).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn runs_before_body_consumption() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::Duration;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let (observed, released) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut received = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let count = socket.read(&mut buffer).unwrap();
            assert_ne!(count, 0, "request should complete before response headers");
            received.extend_from_slice(&buffer[..count]);
            if let Some(end) = received.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&received[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                if received.len() >= end + 4 + length {
                    break;
                }
            }
        }

        let body = response().to_string();
        socket
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nx-fixture-observed: before-body\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        released.recv_timeout(Duration::from_secs(2)).expect("observer should run before body");
        socket.write_all(body.as_bytes()).unwrap();
    });

    let observer: ResponseObserver = Arc::new(move |headers| {
        assert_eq!(headers["x-fixture-observed"], "before-body");
        observed.send(()).unwrap();
    });
    let client = client(&base_url, true).with_response_observer(observer);
    assert_completion(&generate(&client, false).await.expect("generation should succeed"));
    server.join().expect("fixture server should complete");
}
