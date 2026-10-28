//! Raw-value assertions use explicit canonical reads while normal pages stay bounded.
use sailry_client::{Client, conversation::details::Read};
use sailry_protocol::{ErrorCode, Fault};
use sailry_protocol::{
    conversation::{Entry, Page, Part},
    tool::detail::{Detail, Reference},
};
use std::sync::Arc;

pub async fn read(client: &Client, reference: Reference) -> Result<Arc<Detail>, Fault> {
    match client.detail(reference).await? {
        Read::Ready(detail) => Ok(detail.detail),
        Read::TooLarge(limit) => Err(Fault::new(ErrorCode::InvalidRequest, limit.message())),
        Read::Superseded => Err(Fault::new(
            ErrorCode::Cancelled,
            "tool detail read was superseded",
        )),
    }
}

pub async fn tool_detail(client: &Client, page: &Page, entry: &Entry, index: usize) -> Detail {
    let (call, summary) = match &entry.parts[index] {
        Part::ToolCall { id, summary, .. } | Part::ToolResult { id, summary, .. } => (id, summary),
        _ => panic!("tool part expected"),
    };
    read(
        client,
        Reference {
            session: page.session,
            turn: entry.turn,
            entry: entry.id.clone(),
            index,
            call: call.clone(),
            revision: summary.revision.clone(),
        },
    )
    .await
    .unwrap()
    .as_ref()
    .clone()
}

pub async fn tool_details(client: &Client, page: &Page) -> Vec<(String, Detail)> {
    let mut details = Vec::new();
    for entry in &page.entries {
        for (index, part) in entry.parts.iter().enumerate() {
            if let Part::ToolResult { name, .. } = part {
                details.push((name.clone(), tool_detail(client, page, entry, index).await));
            }
        }
    }
    details
}

pub async fn tool_calls(client: &Client, page: &Page) -> Vec<(String, Detail)> {
    let mut details = Vec::new();
    for entry in &page.entries {
        for (index, part) in entry.parts.iter().enumerate() {
            if let Part::ToolCall { name, .. } = part {
                details.push((name.clone(), tool_detail(client, page, entry, index).await));
            }
        }
    }
    details
}
