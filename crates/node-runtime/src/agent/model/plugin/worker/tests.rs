use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use goose_provider_types::{conversation::message::Message, model::ModelConfig};
use rmcp::model::Tool;
use sailry_protocol::{
    Effort,
    plugin::model_adapter::{Capabilities, Dialect, wire},
};
use serde_json::json;

#[tokio::test]
async fn preserves_large_inline_payloads() {
    let entry = "dev.sailry.platform/adapter.js".to_owned();
    let bundle = Bundle {
        entry: entry.clone(),
        files: [(entry, "export function prepare(input){return {method:'POST',url:'https://example.test/models',body:JSON.stringify(input)}}".into())].into(),
    };
    let data: Vec<u8> = (0..2_126_177).map(|index| (index % 251) as u8).collect();
    let messages = [Message::user()
        .with_text("Inspect this attachment")
        .with_image(STANDARD.encode(data), "image/png")];
    let tools: Vec<_> = (0..128)
        .map(|index| {
            Tool::new(
                format!("tool_{index}"),
                "Read a scoped resource",
                serde_json::from_value::<serde_json::Map<String, Value>>(json!({
                    "type":"object","properties":{"path":{"type":"string"}},"required":["path"]
                }))
                .unwrap(),
            )
        })
        .collect();
    let input = super::super::request::convert(
        &ModelConfig::new("example"),
        "",
        &messages,
        &tools,
        "https://example.test",
        "example",
        Effort::Default,
        Default::default(),
        &Capabilities {
            vision: true,
            tools: true,
            ..Default::default()
        },
        Dialect::Generic,
    )
    .unwrap();
    let stop = CancellationToken::new();
    let worker = Worker::new(bundle, &stop, usize::MAX).await.unwrap();
    let prepared: wire::Prepared = worker.call("prepare", &input).await.unwrap();
    let body: Value = serde_json::from_str(prepared.body.as_deref().unwrap()).unwrap();
    assert_eq!(body, serde_json::to_value(&input).unwrap());
    assert_eq!(body["tools"].as_array().unwrap().len(), 128);
    stop.cancel();
    assert_eq!(
        worker
            .call::<Value>("prepare", &input)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
}
