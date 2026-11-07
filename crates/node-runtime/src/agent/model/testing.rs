use super::types::BlockKind;
use super::*;
use futures::StreamExt;
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[path = "factory/tests/server.rs"]
mod server;

pub(crate) async fn collect(mut stream: ModelStream) -> Result<(Message, Usage), Error> {
    let mut blocks = BTreeMap::new();
    let mut usage = Usage::default();
    let mut message = Message::assistant().with_generated_id();
    while let Some(event) = stream.next().await {
        match event? {
            Event::TextDelta { index, text } => {
                let block =
                    blocks
                        .entry((index, BlockKind::Text))
                        .or_insert_with(|| Content::Text {
                            text: String::new(),
                        });
                if let Content::Text { text: accumulated } = block {
                    accumulated.push_str(&text);
                }
            }
            Event::ReasoningDelta {
                index,
                text,
                signature,
            } => {
                let block = blocks
                    .entry((index, BlockKind::Thinking))
                    .or_insert_with(|| Content::Thinking {
                        text: String::new(),
                        signature: None,
                    });
                if let Content::Thinking {
                    text: accumulated,
                    signature: replay,
                } = block
                {
                    if let Some(text) = text {
                        accumulated.push_str(&text);
                    }
                    if let Some(signature) = signature {
                        replay.get_or_insert_with(String::new).push_str(&signature);
                    }
                }
            }
            Event::RedactedThinking { index, data } => {
                blocks.insert(
                    (index, BlockKind::RedactedThinking),
                    Content::RedactedThinking { data },
                );
            }
            Event::ToolCall { index, call } => {
                blocks.insert((index, BlockKind::ToolCall), Content::ToolCall(call));
            }
            Event::Usage(current) => usage = current,
            Event::Finish(finish) => {
                message.metadata.output_token_limit_reached = finish == Finish::Length;
            }
            Event::ToolCallDelta { .. } => {}
        }
    }
    message.content = blocks.into_values().collect();
    Ok((message, usage))
}

pub(crate) async fn roundtrip(message: &Message) -> Message {
    use crate::agent::{canonical::Record, engine::history};
    let database = turso::Builder::new_local(":memory:").build().await.unwrap();
    let connection = database.connect().unwrap();
    let record = history::capture(
        &connection,
        sailry_protocol::TurnId::new(),
        message,
        &BTreeMap::new(),
    )
    .await
    .unwrap();
    let serialized = serde_json::to_value(&record).unwrap();
    let record: Record = serde_json::from_value(serialized).unwrap();
    history::project(&connection, &record)
        .await
        .unwrap()
        .unwrap()
}

pub(crate) async fn capture(api: ModelApi, input: Request) -> Value {
    let server = server::Server::start(vec!["data: [DONE]\n\n".into()]).await;
    let provider: Provider = serde_json::from_value(json!({
        "id":sailry_protocol::ProviderId::new(),"revision":1,"name":"Fixture",
        "api":api,"authentication":"api_key","endpoint":server.endpoint,
        "enabled":true,"models":[],"default_model":input.model,"credential":null,
    }))
    .unwrap();
    let model = factory::native(
        &registry::Registry::default(),
        &provider,
        &input.model,
        sailry_protocol::SessionId::new(),
        "fixture-key",
        HeaderMap::new(),
    )
    .unwrap();
    if let Ok(mut stream) = model.call(input, CancellationToken::new()).await {
        while stream.next().await.is_some() {}
    }
    let captured = server.finish().await;
    assert_eq!(captured.len(), 1);
    assert!(!captured[0].path.is_empty());
    assert!(captured[0].headers.contains_key("content-type"));
    captured.into_iter().next().unwrap().body
}
