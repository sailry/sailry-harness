use crate::agent::canonical::{
    Block, BlockKind, Kind, Message, Record, Role, ToolFormat, ToolRequest, ToolResponse,
    ToolStatus,
};
use sailry_protocol::TurnId;
use serde_json::Value;

pub(in crate::store::agent) fn record(author: &str, kind: Kind) -> Record {
    let mut record = Record::new(TurnId::new(), kind);
    record.provenance.author = author.into();
    record
}

pub(in crate::store::agent) fn message(
    author: &str,
    role: Role,
    content: Vec<BlockKind>,
) -> Record {
    record(
        author,
        Kind::Message(Message {
            role,
            part_count: content.len(),
            content: content
                .into_iter()
                .enumerate()
                .map(|(index, kind)| Block { index, kind })
                .collect(),
            ..Default::default()
        }),
    )
}

pub(in crate::store::agent) fn request(id: &str, name: &str, arguments: Value) -> ToolRequest {
    ToolRequest {
        id: id.into(),
        name: name.into(),
        arguments,
        provider: None,
        tool_meta: None,
        error: None,
    }
}

pub(in crate::store::agent) fn response(id: &str, name: &str, value: Value) -> ToolResponse {
    ToolResponse {
        id: id.into(),
        name: name.into(),
        status: ToolStatus::Success,
        revision: blake3::hash(value.to_string().as_bytes())
            .to_hex()
            .to_string(),
        result: value.into(),
        attachments: Vec::new(),
        provider: None,
        format: ToolFormat::Json,
        meta: None,
    }
}
