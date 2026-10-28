//! Keep signed content and public native tools without retaining replay envelopes.
use super::*;
use adk_core::Part;

pub(super) fn ordinary(mut part: &Part) -> &Part {
    while let Part::ProviderData { provider_data } = part {
        part = &provider_data.parts[0];
    }
    part
}

pub(super) fn private(value: &Value) -> bool {
    value.get("encrypted_content").is_some()
        || value["type"] == "reasoning"
        || value["type"] == "compaction"
        || value
            .as_array()
            .is_some_and(|items| items.iter().any(private))
}

pub(super) fn capture(content: &Content, native: &Value) -> Result<Option<BlockKind>, Fault> {
    let values = native["content"]
        .as_array()
        .ok_or_else(|| Fault::invalid("native thinking content is invalid"))?;
    let mut cursor = 0;
    let mut positions = Vec::new();
    for (index, value) in values.iter().enumerate() {
        if value["type"] == "redacted_thinking" {
            continue;
        }
        if let Some(offset) = content.parts[cursor..]
            .iter()
            .position(|part| matches(value, ordinary(part)))
        {
            cursor += offset;
            positions.push((index, cursor));
            cursor += 1;
        }
    }
    let mut data = Vec::new();
    let mut before = Vec::new();
    for (index, value) in values
        .iter()
        .enumerate()
        .filter(|(_, value)| value["type"] == "redacted_thinking")
    {
        data.push(
            value["data"]
                .as_str()
                .ok_or_else(|| Fault::invalid("redacted thinking data is invalid"))?
                .into(),
        );
        before.push(
            positions
                .iter()
                .find(|(position, _)| *position > index)
                .map(|(_, slot)| *slot),
        );
    }
    Ok((!data.is_empty()).then_some(BlockKind::RedactedThinking { data, before }))
}

fn matches(value: &Value, part: &Part) -> bool {
    match part {
        Part::Text { text } => value["type"] == "text" && value["text"] == *text,
        Part::Thinking {
            thinking,
            signature,
        } => {
            value["type"] == "thinking"
                && value["thinking"] == *thinking
                && value.get("signature").and_then(Value::as_str) == signature.as_deref()
        }
        Part::FunctionCall { id: Some(id), .. } => {
            value["type"] == "tool_use" && value["id"] == *id
        }
        Part::ServerToolCall { server_tool_call } => {
            value.get("id").is_some() && value.get("id") == server_tool_call.get("id")
        }
        Part::ServerToolResponse {
            server_tool_response,
        } => {
            value == server_tool_response
                || (value.get("tool_use_id").is_some()
                    && value.get("tool_use_id") == server_tool_response.get("tool_use_id"))
        }
        _ => false,
    }
}

pub(super) async fn restore(
    db: &Connection,
    message: &Message,
    data: &[String],
    before: &[Option<usize>],
) -> Result<Value, Fault> {
    let mut content = Vec::new();
    for block in &message.content {
        for (position, data) in data.iter().enumerate() {
            if before.get(position).copied().flatten() == Some(block.index) {
                content.push(json!({"type":"redacted_thinking","data":data}));
            }
        }
        let value = match &block.kind {
            BlockKind::Text { text } => Some(json!({"type":"text","text":text})),
            BlockKind::Thinking {
                text,
                signature: Some(signature),
            } => Some(json!({"type":"thinking","thinking":text,"signature":signature})),
            BlockKind::ToolRequest(call) => Some(
                json!({"type":"tool_use","id":call.id,"name":call.name,"input":call.arguments}),
            ),
            BlockKind::ServerCall { value } => {
                let mut value = blobs::hydrate(db, value).await?;
                if let Some(object) = value.as_object_mut() {
                    object
                        .entry("type")
                        .or_insert_with(|| json!("server_tool_use"));
                }
                Some(value)
            }
            BlockKind::ServerResponse { value } => {
                let mut value = blobs::hydrate(db, value).await?;
                if let Some(object) = value.as_object_mut() {
                    object
                        .entry("type")
                        .or_insert_with(|| json!("web_search_tool_result"));
                }
                Some(value)
            }
            _ => None,
        };
        content.extend(value);
    }
    for (position, data) in data.iter().enumerate() {
        if before.get(position).copied().flatten().is_none() {
            content.push(json!({"type":"redacted_thinking","data":data}));
        }
    }
    Ok(json!({"type":"anthropic_message","content":content}))
}
