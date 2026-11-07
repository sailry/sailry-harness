use super::super::types::{Config, Content as Part, Error, ErrorKind, Event, Message, Request};
use super::{framing, request as translation, response, transport, worker};
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock as Content, Tool};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Effort, ErrorCode, Secret,
    plugin::model_adapter::{
        Capabilities, Dialect, Framing,
        wire::{self, Delta, Finish},
    },
};
use serde_json::json;

mod exposure;
mod input_evidence;

mod request {
    use super::*;
    pub(super) use translation::Context;

    pub(super) fn convert(
        config: &Config,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
        context: Context<'_>,
    ) -> Result<wire::Request, sailry_protocol::Fault> {
        let request = config.request(
            system.to_owned(),
            messages.to_vec(),
            tools.iter().cloned().map(Into::into).collect(),
        );
        translation::convert(&request, context)
    }
}

fn capabilities() -> Capabilities {
    Capabilities {
        vision: true,
        files: true,
        tools: true,
        reasoning: true,
    }
}

#[test]
fn retains_neutral_messages_and_schemas() {
    let messages = [
        Message::user()
            .with_text("task")
            .with_image("aW1hZ2U=", "image/png"),
        Message::assistant()
            .with_thinking("reason", "signature")
            .with_tool_request(
                "call-1",
                Ok(CallToolRequestParams::new("lookup")
                    .with_arguments(serde_json::from_value(json!({"path":"file"})).unwrap())),
            ),
        Message::user().with_tool_response(
            "call-1",
            Ok(CallToolResult::success(vec![Content::text("result")])),
        ),
    ];
    let config = Config {
        model_name: "captured".into(),
        max_tokens: Some(128),
        request_params: [
            ("top_p".into(), json!(0.9)),
            ("custom".into(), json!("frozen")),
            ("thinking_effort".into(), json!("high")),
            ("budget_tokens".into(), json!(4096)),
            ("preserve_thinking_context".into(), json!(true)),
            ("stop".into(), json!(["END"])),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let tools = [Tool::new(
        "lookup",
        "Read",
        serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
            json!({"type":"object","properties":{"path":{"type":"string"}}}),
        )
        .unwrap(),
    )];
    for dialect in [Dialect::Generic, Dialect::OpenAi, Dialect::Gemini] {
        let input = request::convert(
            &config,
            "instructions",
            &messages,
            &tools,
            request::Context {
                endpoint: "http://localhost",
                session: "session",
                effort: Effort::High,
                features: Default::default(),
                capabilities: &capabilities(),
                dialect,
            },
        )
        .unwrap();
        assert_eq!(input.model, "captured");
        assert_eq!(input.system.as_deref(), Some("instructions"));
        assert_eq!(input.messages.len(), 3);
        assert_eq!(input.tools[0].name, "lookup");
        assert_eq!(
            input.tools[0].parameters["properties"]["path"]["type"],
            "string"
        );
        assert_eq!(input.options["custom"], "frozen");
        assert_eq!(input.options.len(), 1);
        assert_eq!(input.top_p, Some(0.9));
        assert_eq!(input.stop, vec!["END"]);
        assert!(matches!(input.tool_choice, wire::Choice::Auto));
        assert!(
            matches!(&input.messages[1], wire::Message::Assistant {parts} if matches!(&parts[0],wire::Part::Reasoning {replay:Some(signature),..} if signature=="signature"))
        );
        assert!(
            matches!(&input.messages[2], wire::Message::Tool {call_id,is_error:false,..} if call_id=="call-1")
        );
    }
}

#[test]
fn rejects_undeclared_capabilities() {
    for (messages, effort) in [
        (vec![Message::user().with_text("task")], Effort::High),
        (
            vec![Message::user().with_image("aW1hZ2U=", "image/png")],
            Effort::Default,
        ),
        (
            vec![Message::assistant().with_thinking("reason", "signature")],
            Effort::Default,
        ),
        (
            vec![Message::user().with_document("cGRm", "application/pdf", None)],
            Effort::Default,
        ),
        (
            vec![
                Message::assistant()
                    .with_tool_request("call", Ok(CallToolRequestParams::new("lookup"))),
            ],
            Effort::Default,
        ),
    ] {
        let error = request::convert(
            &Config {
                model_name: "captured".into(),
                ..Default::default()
            },
            "",
            &messages,
            &[],
            request::Context {
                endpoint: "http://localhost",
                session: "session",
                effort,
                features: Default::default(),
                capabilities: &Capabilities {
                    vision: false,
                    files: false,
                    tools: false,
                    reasoning: false,
                },
                dialect: Dialect::Generic,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, sailry_protocol::ErrorCode::InvalidRequest);
    }
}

#[test]
fn joins_calls_and_keeps_unknown_usage() {
    let mut output = response::Accumulator::default();
    let chunks = output
        .push(
            vec![
                Delta::Text {
                    text: "Hello".into(),
                },
                Delta::ToolCall {
                    index: 0,
                    id: Some("call".into()),
                    name: Some("look".into()),
                    arguments_delta: Some("{\"path\":".into()),
                },
                Delta::ToolCall {
                    index: 0,
                    id: None,
                    name: Some("up".into()),
                    arguments_delta: Some("\"file\"}".into()),
                },
                Delta::Usage {
                    input_tokens: None,
                    output_tokens: Some(3),
                    cached_input_tokens: None,
                    reasoning_tokens: None,
                },
                Delta::Finish {
                    reason: Finish::ToolCalls,
                },
            ],
            &[],
        )
        .unwrap();
    assert_eq!(chunks.len(), 3);
    assert!(
        !chunks
            .iter()
            .any(|event| matches!(event, Event::ToolCall { .. }))
    );
    let final_chunks = output.finish("captured").unwrap();
    let Event::ToolCall { call, .. } = &final_chunks[0] else {
        panic!("tool request expected")
    };
    assert_eq!(call.id, "call");
    assert_eq!(call.name, "lookup");
    assert_eq!(call.arguments["path"], "file");
    let Event::Usage(usage) = &final_chunks[1] else {
        panic!("usage expected")
    };
    assert_eq!(usage.input, None);
    assert_eq!(usage.output, Some(3));
    assert_eq!(usage.total, None);
}

#[test]
fn marks_buffered_deltas_as_output() {
    let reasoning = |text: Option<&str>, replay: Option<&str>| Delta::Reasoning {
        text: text.map(Into::into),
        replay: replay.map(Into::into),
    };
    for delta in [
        reasoning(Some("Plan"), None),
        reasoning(None, Some("signature")),
        Delta::ToolCall {
            index: 0,
            id: Some("call".into()),
            name: None,
            arguments_delta: None,
        },
        Delta::Text { text: "A".into() },
    ] {
        assert!(response::output(&delta));
    }
    for delta in [
        reasoning(None, None),
        reasoning(Some(""), Some("")),
        Delta::Text {
            text: String::new(),
        },
        Delta::Usage {
            input_tokens: Some(1),
            output_tokens: None,
            cached_input_tokens: None,
            reasoning_tokens: None,
        },
        Delta::Finish {
            reason: Finish::Stop,
        },
    ] {
        assert!(!response::output(&delta));
    }
}

#[test]
fn joins_replay_fragments_before_signing_reasoning() {
    let mut accumulator = response::Accumulator::default();
    let events = accumulator
        .push(
            vec![
                Delta::Reasoning {
                    text: Some("Plan".into()),
                    replay: Some("first-".into()),
                },
                Delta::Reasoning {
                    text: None,
                    replay: Some("second".into()),
                },
            ],
            &[],
        )
        .unwrap();
    let [
        Event::ReasoningDelta {
            index,
            text: Some(text),
            signature: None,
        },
    ] = events.as_slice()
    else {
        panic!("only unsigned reasoning should escape before the boundary")
    };
    assert_eq!(text, "Plan");
    let completed = accumulator
        .push(
            vec![Delta::Text {
                text: "Answer".into(),
            }],
            &[],
        )
        .unwrap();
    let [
        Event::ReasoningDelta {
            index: completed_index,
            text: None,
            signature: Some(signature),
        },
        Event::TextDelta { .. },
    ] = completed.as_slice()
    else {
        panic!("complete signature must precede the answer")
    };
    assert_eq!(completed_index, index);
    assert_eq!(signature, "first-second");
}

#[test]
fn retains_optional_reasoning_counts() {
    for tokens in [None, Some(0), Some(2)] {
        let mut output = response::Accumulator::default();
        output
            .push(
                vec![
                    Delta::Usage {
                        input_tokens: Some(12),
                        output_tokens: Some(3),
                        cached_input_tokens: None,
                        reasoning_tokens: tokens,
                    },
                    Delta::Finish {
                        reason: Finish::Stop,
                    },
                ],
                &[],
            )
            .unwrap();
        let chunks = output.finish("fixture").unwrap();
        let Event::Usage(usage) = &chunks[0] else {
            panic!("usage expected")
        };
        assert_eq!(usage.reasoning, tokens);
        assert_eq!((usage.input, usage.output), (Some(12), Some(3)));
    }
}

#[test]
fn marks_invalid_arguments_without_substitution() {
    let mut output = response::Accumulator::default();
    output
        .push(
            vec![
                Delta::ToolCall {
                    index: 0,
                    id: Some("call".into()),
                    name: Some("lookup".into()),
                    arguments_delta: Some("{broken".into()),
                },
                Delta::Finish {
                    reason: Finish::ToolCalls,
                },
            ],
            &[],
        )
        .unwrap();
    let chunks = output.finish("model").unwrap();
    let Event::ToolCall { call, .. } = &chunks[0] else {
        panic!("tool request expected")
    };
    assert!(call.error.is_some());
}

#[test]
fn keeps_failures_and_private_diagnostics() {
    let error = response::Accumulator::default()
        .finish("model")
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidResponse);
    assert!(!super::super::errors::network(&error));
    let secret = Secret::new("private-token".into());
    let error = response::classified(
        wire::Class::RateLimited,
        "private-token quota",
        Some(2000),
        &[secret],
    );
    assert_eq!(
        error,
        Error {
            kind: ErrorKind::RateLimited,
            message: "[redacted] quota".into(),
            retry_after: Some(std::time::Duration::from_secs(2))
        }
    );
}

#[test]
fn keeps_provider_rejections_distinct_from_network() {
    for class in [wire::Class::Unknown, wire::Class::Unavailable] {
        let error = response::classified(class, "provider unavailable", None, &[]);
        assert!(!super::super::errors::network(&error));
        // Preserve the existing transient retry policy for these responses.
        assert!(error.retryable());
    }
    assert!(super::super::errors::network(
        &transport::Failure::Network.model()
    ));
    let error = transport::Failure::Network.fault();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(error.details, Some(sailry_protocol::FaultDetails::Network));
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
