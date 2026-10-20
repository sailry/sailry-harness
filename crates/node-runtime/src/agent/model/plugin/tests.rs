use super::{framing, request, response, transport, worker};
use adk_core::{Content, GenerateContentConfig, LlmRequest, Part};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Effort, Secret,
    plugin::model_adapter::{
        Capabilities, Framing,
        wire::{self, Delta, Finish},
    },
};
use serde_json::json;
use tokio::sync::mpsc;

fn capabilities() -> Capabilities {
    Capabilities {
        vision: true,
        files: true,
        tools: true,
        reasoning: true,
    }
}

#[test]
fn retains_multimodal_history_and_options() {
    let mut input = LlmRequest::new(
        "example",
        vec![
            Content::new("system").with_text("system instruction"),
            Content::new("user").with_text("task"),
            Content {
                role: "model".into(),
                parts: vec![
                    Part::Thinking {
                        thinking: "summary".into(),
                        signature: Some("opaque".into()),
                    },
                    Part::FunctionCall {
                        name: "lookup".into(),
                        args: json!({"path":"a"}),
                        id: Some("call-1".into()),
                        thought_signature: None,
                    },
                ],
            },
            Content {
                role: "function".into(),
                parts: vec![Part::FunctionResponse {
                    id: Some("call-1".into()),
                    annotations: None,
                    function_response: adk_core::FunctionResponseData {
                        name: "lookup".into(),
                        response: json!({"result":true}),
                        inline_data: vec![adk_core::InlineDataPart {
                            mime_type: "image/png".into(),
                            data: vec![1, 2],
                            uri: None,
                            annotations: None,
                        }],
                        file_data: Vec::new(),
                    },
                }],
            },
        ],
    );
    input.config = Some(GenerateContentConfig {
        temperature: Some(0.5),
        top_k: Some(12),
        seed: Some(7),
        stop_sequences: vec!["end".into()],
        response_schema: Some(json!({"type":"object"})),
        ..Default::default()
    });
    input.tools.insert(
        "lookup".into(),
        json!({"name":"lookup","description":"Read","parameters":{"type":"object"}}),
    );
    let value = request::convert(
        input,
        "http://localhost:8080",
        "session",
        Effort::High,
        &capabilities(),
        true,
    )
    .unwrap();
    assert_eq!(value.system.as_deref(), Some("system instruction"));
    assert_eq!(value.messages.len(), 3);
    let wire::Message::Tool { call_id, parts, .. } = &value.messages[2] else {
        panic!("tool result expected")
    };
    assert_eq!(call_id, "call-1");
    assert_eq!(parts.len(), 2);
    assert!(
        matches!(&parts[1],wire::Part::Image { data:wire::Image::Inline { data_base64 },.. } if data_base64=="AQI=")
    );
    assert_eq!(value.options["seed"], 7);
    assert_eq!(value.options["top_k"], 12);
    assert_eq!(value.options["response_schema"], json!({"type":"object"}));
    assert_eq!(value.stop, ["end"]);
    assert_eq!(value.tools[0].parameters, json!({"type":"object"}));
}

#[test]
fn rejects_unrepresentable_parts_without_dropping() {
    for part in [
        Part::ServerToolCall {
            server_tool_call: json!({}),
        },
        Part::FunctionCall {
            id: None,
            name: "call".into(),
            args: json!({}),
            thought_signature: None,
        },
        Part::InlineData {
            mime_type: "image/png".into(),
            data: vec![1],
            uri: None,
            annotations: None,
        },
    ] {
        let input = LlmRequest::new(
            "example",
            vec![Content {
                role: "user".into(),
                parts: vec![part],
            }],
        );
        assert!(
            request::convert(
                input,
                "http://localhost:8080",
                "session",
                Effort::Default,
                &Capabilities::default(),
                false
            )
            .is_err()
        );
    }
}

#[test]
fn joins_calls_and_usage_before_execution() {
    let mut output = response::Accumulator::default();
    let pieces = output
        .push(
            vec![
                Delta::ToolCall {
                    index: 9,
                    id: Some("call-9".into()),
                    name: Some("look".into()),
                    arguments_delta: Some("{\"q\":".into()),
                },
                Delta::ToolCall {
                    index: 9,
                    id: None,
                    name: Some("up".into()),
                    arguments_delta: Some("\"value\"}".into()),
                },
                Delta::Usage {
                    input_tokens: Some(10),
                    output_tokens: Some(3),
                    cached_input_tokens: Some(4),
                    reasoning_tokens: Some(1),
                },
                Delta::Finish {
                    reason: Finish::ToolCalls,
                },
            ],
            true,
            &[],
        )
        .unwrap();
    assert!(pieces.iter().all(|response| response.content.is_none()));
    let final_output = output.finish(&["lookup".into()].into()).unwrap();
    assert!(!final_output.partial);
    let usage = final_output.usage_metadata.unwrap();
    assert_eq!(usage.total_token_count, 13);
    assert_eq!(usage.cache_read_input_token_count, Some(4));
    let parts = final_output.content.unwrap().parts;
    assert!(
        matches!(&parts[0],Part::FunctionCall { id:Some(id),name,args,.. } if id=="call-9" && name=="lookup" && args==&json!({"q":"value"}))
    );
}

#[test]
fn rejects_incomplete_streams_and_private_errors() {
    let mut output = response::Accumulator::default();
    output
        .push(
            vec![Delta::Text {
                text: "partial".into(),
            }],
            true,
            &[],
        )
        .unwrap();
    assert!(
        output
            .finish(&Default::default())
            .unwrap_err()
            .retry
            .should_retry
    );
    let error = response::classified(
        wire::Class::RateLimited,
        "echo private-token",
        Some(20),
        Some(429),
        &[Secret::new("private-token".into())],
    );
    assert!(error.retry.should_retry);
    assert!(!error.to_string().contains("private-token"));
    assert_eq!(error.retry.retry_after_ms, Some(20));
    let error = response::classified(wire::Class::Unavailable, "failed", None, None, &[]);
    assert!(error.retry.should_retry);
    let error = response::classified(wire::Class::Unknown, "No upstream code", None, None, &[]);
    assert_eq!(error.category, adk_core::ErrorCategory::Unavailable);
    assert!(error.retry.should_retry);
    let error = response::resource();
    assert_eq!(error.category, adk_core::ErrorCategory::Internal);
    assert_eq!(error.code, "model.plugin.resource_budget");
    assert!(!error.retry.should_retry);
    let error = response::translation(sailry_protocol::Fault::new(
        sailry_protocol::ErrorCode::Unavailable,
        "synthetic runtime failure",
    ));
    assert_eq!(error.category, adk_core::ErrorCategory::Internal);
    assert!(!error.retry.should_retry);
}

#[test]
fn marks_invalid_arguments_without_substitution() {
    for raw in ["", " ", "{", "[]", "42", "null"] {
        let mut output = response::Accumulator::default();
        let partials = output
            .push(
                vec![
                    Delta::ToolCall {
                        index: 0,
                        id: Some("call".into()),
                        name: Some("lookup".into()),
                        arguments_delta: Some(raw.into()),
                    },
                    Delta::Finish {
                        reason: Finish::ToolCalls,
                    },
                ],
                true,
                &[],
            )
            .unwrap();
        assert!(partials.iter().all(|response| response.content.is_none()));
        let response = output.finish(&["lookup".into()].into()).unwrap();
        assert!(!response.partial);
        assert!(response.turn_complete);
        let content = response.content.unwrap();
        let [Part::FunctionCall { id, name, args, .. }] = content.parts.as_slice() else {
            panic!("one marked call expected")
        };
        assert_eq!(id.as_deref(), Some("call"));
        assert_eq!(name, "lookup");
        assert!(adk_core::invalid_tool_arguments_reason(args).is_some());
        assert_eq!(args[adk_core::INVALID_TOOL_ARGUMENTS_KEY]["raw"], raw);
        assert_ne!(args, &json!({}));
    }
}

#[test]
fn validates_identity_before_marking_arguments() {
    for (id, name) in [("", "lookup"), ("call", ""), ("call", "other")] {
        let mut output = response::Accumulator::default();
        output
            .push(
                vec![
                    Delta::ToolCall {
                        index: 0,
                        id: Some(id.into()),
                        name: Some(name.into()),
                        arguments_delta: Some("{".into()),
                    },
                    Delta::Finish {
                        reason: Finish::ToolCalls,
                    },
                ],
                true,
                &[],
            )
            .unwrap();
        let error = output.finish(&["lookup".into()].into()).unwrap_err();
        assert_eq!(error.code, "model.plugin.invalid_translation");
        assert_eq!(
            error.message,
            "model returned an invalid tool-call identity"
        );
        assert!(!error.retry.should_retry);
    }
}

#[tokio::test]
async fn keeps_translation_state_without_host_imports() {
    let entry = "dev.sailry.platform/adapter.js".to_owned();
    let bundle = crate::plugins::script::Bundle {
        entry: entry.clone(),
        files: [(
            entry,
            "let count=0; export function decode(input){return {count:++count,input}}".into(),
        )]
        .into(),
    };
    let stop = CancellationToken::new();
    let worker = worker::Worker::new(bundle, &stop, 4096).await.unwrap();
    let first: serde_json::Value = worker.call("decode", &json!({"value":1})).await.unwrap();
    let second: serde_json::Value = worker.call("decode", &json!({"value":2})).await.unwrap();
    assert_eq!(first["count"], 1);
    assert_eq!(second["count"], 2);
    stop.cancel();
    assert!(
        worker
            .call::<serde_json::Value>("decode", &json!({}))
            .await
            .is_err()
    );
}

#[test]
fn frames_transport_without_header_disclosure() {
    let mut decoder = framing::Decoder::new(Framing::Sse, 1024);
    assert_eq!(
        decoder
            .push(b": ping\n\ndata: {\"a\":1}\n\n", false)
            .unwrap()
            .len(),
        1
    );
    let credentials = transport::Credentials {
        headers: Default::default(),
        access: None,
        key: None,
        secrets: vec![Secret::new("private-token".into())],
    };
    assert!(transport::public("safe response", &credentials).is_ok());
    assert!(transport::public("private-token", &credentials).is_err());
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "retry-after",
        reqwest::header::HeaderValue::from_static("private-token"),
    );
    assert!(transport::headers(&headers, &credentials).is_err());
}

#[tokio::test]
async fn emits_only_requested_stream_events() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::mpsc,
    };
    for streaming in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
            }
            let body = r#"{"text":"complete"}"#;
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        });
        let received = reqwest::Client::new()
            .get(format!("http://{address}/result"))
            .send()
            .await
            .unwrap();
        let entry = "dev.sailry.platform/adapter.js".to_owned();
        let bundle=crate::plugins::script::Bundle { entry:entry.clone(),files:[(entry,r#"
export function decode(input) {
  if(!input.done || input.events.length!==1) throw new Error('Expected complete JSON');
  return {state:null,deltas:[{kind:'text',text:JSON.parse(input.events[0].data).text},{kind:'finish',reason:'stop'}]};
}
"#.into())].into() };
        let stop = CancellationToken::new();
        let worker = worker::Worker::new(bundle, &stop, 4096).await.unwrap();
        let (sender, mut receiver) = mpsc::channel(1);
        let producer = tokio::spawn(async move {
            let mut accumulator = response::Accumulator::default();
            let credentials = transport::Credentials {
                headers: Default::default(),
                access: None,
                key: None,
                secrets: Vec::new(),
            };
            super::produce(
                received,
                worker,
                Framing::Json,
                200,
                Default::default(),
                super::StreamContext {
                    credentials: &credentials,
                    stop: &stop,
                    streaming,
                    sender: &sender,
                },
                &mut accumulator,
            )
            .await
            .unwrap();
            sender
                .send(Ok(accumulator.finish(&Default::default()).unwrap()))
                .await
                .unwrap();
        });
        let mut output = Vec::new();
        while let Some(value) = receiver.recv().await {
            output.push(value.unwrap());
        }
        producer.await.unwrap();
        server.await.unwrap();
        if streaming {
            assert!(
                output
                    .iter()
                    .any(|response| response.partial && response.content.is_none())
            );
            assert!(
                output
                    .iter()
                    .any(|response| response.partial && response.content.is_some())
            );
        } else {
            assert_eq!(output.len(), 1);
        }
        let final_output = output.last().unwrap();
        assert!(!final_output.partial);
        assert!(final_output.turn_complete);
        assert!(
            matches!(&final_output.content.as_ref().unwrap().parts[0],Part::Text { text } if text=="complete")
        );
    }
}

#[tokio::test]
async fn bounds_requests_before_network_io() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let credentials = transport::Credentials {
        headers: Default::default(),
        access: None,
        key: None,
        secrets: vec![],
    };
    let declaration=serde_json::from_value(json!({"id":"example","script":{"entry":"dev.sailry.platform/adapter.js"},"origins":[],"framing":"json"})).unwrap();
    let prepared=serde_json::from_value(json!({"method":"POST","url":"http://127.0.0.1:9/","headers":{},"body":"x".repeat(transport::REQUEST_BYTES+1),"framing":"json"})).unwrap();
    let error = transport::send(
        &transport::client().unwrap(),
        prepared,
        "http://127.0.0.1:9/",
        &declaration,
        &credentials,
        &CancellationToken::new(),
        usize::MAX,
    )
    .await
    .unwrap_err()
    .fault();
    assert_eq!(error.code, sailry_protocol::ErrorCode::Internal);
    for (status, category) in [
        (401, adk_core::ErrorCategory::Unauthorized),
        (403, adk_core::ErrorCategory::Forbidden),
        (429, adk_core::ErrorCategory::RateLimited),
        (503, adk_core::ErrorCategory::Unavailable),
    ] {
        let error = response::classified(
            response::http_class(status),
            "Synthetic HTTP failure",
            None,
            Some(status),
            &[],
        );
        assert_eq!(error.category, category);
        assert_eq!(error.details.upstream_status_code, Some(status));
        assert_eq!(error.retry.should_retry, matches!(status, 429 | 503));
    }
}

#[tokio::test]
async fn defaults_when_classify_is_not_exported() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::mpsc,
    };
    for status in [401, 429, 503] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 1024];
            let _ = socket.read(&mut bytes).await.unwrap();
            socket.write_all(format!("HTTP/1.1 {status} Synthetic\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        });
        let received = reqwest::Client::new()
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap();
        let entry = "dev.sailry.platform/adapter.js".to_owned();
        let bundle = crate::plugins::script::Bundle {
            entry: entry.clone(),
            files: [(
                entry,
                "export function decode(){throw Error('Should not decode HTTP failure')}".into(),
            )]
            .into(),
        };
        let stop = CancellationToken::new();
        let worker = worker::Worker::new(bundle, &stop, 4096).await.unwrap();
        assert!(!worker.classifies);
        let credentials = transport::Credentials {
            headers: Default::default(),
            access: None,
            key: None,
            secrets: vec![],
        };
        let (sender, _) = mpsc::channel(1);
        let mut accumulator = response::Accumulator::default();
        let error = super::produce(
            received,
            worker,
            Framing::Json,
            status,
            Default::default(),
            super::StreamContext {
                credentials: &credentials,
                stop: &stop,
                streaming: true,
                sender: &sender,
            },
            &mut accumulator,
        )
        .await
        .unwrap_err();
        assert_eq!(error.details.upstream_status_code, Some(status));
        assert_eq!(
            error.category,
            if status == 401 {
                adk_core::ErrorCategory::Unauthorized
            } else if status == 429 {
                adk_core::ErrorCategory::RateLimited
            } else {
                adk_core::ErrorCategory::Unavailable
            }
        );
        server.await.unwrap();
    }
}

#[tokio::test]
async fn finishes_before_the_http_connection_closes() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let stopped = release.clone();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 1024];
        let _ = stream.read(&mut request).await.unwrap();
        let data = r#"[{"kind":"text","text":"done"},{"kind":"usage","input_tokens":11,"output_tokens":3,"cached_input_tokens":4,"reasoning_tokens":1},{"kind":"finish","reason":"stop"}]"#;
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\ndata: {data}\n\n").as_bytes()).await.unwrap();
        stopped.notified().await;
    });
    let received = transport::client()
        .unwrap()
        .get(format!("http://{address}/"))
        .send()
        .await
        .unwrap();
    let entry = "dev.sailry.platform/adapter.js".to_owned();
    let worker = worker::Worker::new(crate::plugins::script::Bundle {
        entry: entry.clone(), files: [(entry, "export function decode(input) {return {state:null,deltas:input.events.flatMap(event=>JSON.parse(event.data))}}".into())].into(),
    }, &CancellationToken::new(), 4096).await.unwrap();
    let stop = CancellationToken::new();
    let credentials = transport::Credentials {
        headers: Default::default(),
        access: None,
        key: None,
        secrets: vec![],
    };
    let (sender, _) = mpsc::channel(1);
    let mut accumulator = response::Accumulator::default();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        super::produce(
            received,
            worker,
            Framing::Sse,
            200,
            Default::default(),
            super::StreamContext {
                credentials: &credentials,
                stop: &stop,
                streaming: false,
                sender: &sender,
            },
            &mut accumulator,
        ),
    )
    .await;
    release.notify_one();
    server.await.unwrap();
    result
        .expect("the declared finish must not wait for HTTP EOF")
        .unwrap();
    let response = accumulator.finish(&Default::default()).unwrap();
    assert!(response.turn_complete);
    let usage = response.usage_metadata.unwrap();
    assert_eq!(usage.prompt_token_count, 11);
    assert_eq!(usage.candidates_token_count, 3);
    assert_eq!(usage.cache_read_input_token_count, Some(4));
    assert_eq!(usage.thinking_token_count, Some(1));
    assert_eq!(
        response.content.unwrap().parts,
        [adk_core::Part::Text {
            text: "done".into()
        }]
    );
}
