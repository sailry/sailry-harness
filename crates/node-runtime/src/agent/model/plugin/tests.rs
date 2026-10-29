use super::{framing, request, response, transport, worker};
use goose_provider_types::{
    conversation::message::{Message, MessageContentBlock},
    errors::ProviderError,
    model::ModelConfig,
};
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock as Content, Tool};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Effort, Secret,
    plugin::model_adapter::{
        Capabilities, Dialect, Framing,
        wire::{self, Delta, Finish},
    },
};
use serde_json::json;

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
    let config = ModelConfig::new("captured")
        .with_max_tokens(Some(128))
        .with_merged_request_params(
            [
                ("top_p".into(), json!(0.9)),
                ("custom".into(), json!("frozen")),
            ]
            .into(),
        );
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
    assert_eq!(chunks.len(), 1);
    let final_chunks = output.finish("captured").unwrap();
    let (message, usage) = final_chunks.last().unwrap();
    let MessageContentBlock::ToolRequest(request) = &message.as_ref().unwrap().content[0] else {
        panic!("tool request expected")
    };
    assert_eq!(request.id, "call");
    let call = request.tool_call.as_ref().unwrap();
    assert_eq!(call.name, "lookup");
    assert_eq!(call.arguments.as_ref().unwrap()["path"], "file");
    let usage = usage.as_ref().unwrap();
    assert_eq!(usage.usage.input_tokens, None);
    assert_eq!(usage.usage.output_tokens, Some(3));
    assert_eq!(usage.usage.total_tokens, None);
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
    let MessageContentBlock::ToolRequest(request) = &chunks[0].0.as_ref().unwrap().content[0]
    else {
        panic!("tool request expected")
    };
    assert!(request.tool_call.is_err());
}

#[test]
fn keeps_failures_and_private_diagnostics() {
    assert!(matches!(
        response::Accumulator::default().finish("model"),
        Err(ProviderError::NetworkError(_))
    ));
    let secret = Secret::new("private-token".into());
    let error = response::classified(
        wire::Class::RateLimited,
        "private-token quota",
        Some(2000),
        Some(429),
        &[secret],
    );
    assert_eq!(
        error,
        ProviderError::RateLimitExceeded {
            details: "[redacted] quota".into(),
            retry_delay: Some(std::time::Duration::from_secs(2))
        }
    );
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
