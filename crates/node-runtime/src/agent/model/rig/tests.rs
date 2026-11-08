use super::RigModel;
use crate::agent::model::{
    registry,
    testing::server::{self, Server},
    types::{
        Content, Error, ErrorKind, Event, Finish, Image, Message, Model, Request, Role, Tool,
        ToolResult,
    },
};
use futures::TryStreamExt;
use reqwest::header::HeaderMap;
use sailry_protocol::{Authentication, SessionId, conversation::ModelApi};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

mod cancellation;
mod cloud;
mod eof;
mod errors;
mod fixtures;
mod framing;
mod gemini;
mod history;
mod parity;
mod provider_contracts;
mod requests;
mod stalled;
mod streaming;

fn provider(api: ModelApi, endpoint: String) -> sailry_protocol::conversation::Provider {
    serde_json::from_value(json!({
        "id":sailry_protocol::ProviderId::new(),"revision":1,"name":"Captured provider",
        "api":api,"authentication":"api_key","endpoint":endpoint,"enabled":true,
        "models":[],"default_model":"fixture","credential":null
    }))
    .unwrap()
}

fn request(model: &str) -> Request {
    Request {
        model: model.into(),
        system: "Captured instructions".into(),
        messages: vec![Message {
            role: Role::User,
            content: vec![Content::Text {
                text: "Read the file".into(),
            }],
            ..Message::default()
        }],
        tools: vec![Tool {
            name: "read".into(),
            description: Some("Read a file".into()),
            parameters: json!({
                "type":"object","properties":{"path":{"type":"string"}},"required":["path"]
            }),
        }],
        max_tokens: Some(512),
        ..Default::default()
    }
}

fn native(
    provider: &sailry_protocol::conversation::Provider,
    model: &str,
    key: &str,
    headers: HeaderMap,
) -> RigModel {
    let _ = rustls::crypto::ring::default_provider().install_default();
    RigModel::new(
        &registry::Registry::default(),
        provider,
        model,
        SessionId::new(),
        key,
        headers,
    )
    .unwrap()
}

async fn collect(
    model: &RigModel,
    request: Request,
    cancel: CancellationToken,
) -> Result<Vec<Event>, Error> {
    tokio::time::timeout(Duration::from_secs(10), async {
        model.call(request, cancel).await?.try_collect().await
    })
    .await
    .expect("model fixture deadline")
}

fn text(events: &[Event]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            Event::TextDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn reasoning(events: &[Event]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ReasoningDelta { text, .. } => text.as_deref(),
            _ => None,
        })
        .collect()
}

fn result(id: &str) -> Message {
    Message {
        role: Role::User,
        content: vec![Content::ToolResult(ToolResult {
            id: id.into(),
            name: "read".into(),
            result: Ok(rmcp::model::CallToolResult::success(vec![
                rmcp::model::ContentBlock::text("File contents"),
            ])),
            metadata: None,
        })],
        ..Message::default()
    }
}
