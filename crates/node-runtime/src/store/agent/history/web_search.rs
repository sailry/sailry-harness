use sailry_protocol::{ErrorCode, Fault, conversation::Part};
use serde_json::{Value, json};

pub(super) fn call(
    value: &Value,
    display: Option<&sailry_protocol::tool::Display>,
) -> Option<Part> {
    if value["name"] == "web_search" && value.get("input").is_some() {
        return Some(Part::ToolCall {
            presentation: sailry_protocol::tool::Presentation::Summary,
            grouping: Default::default(),
            display: display.cloned().map(Box::new),
            id: Some(value["id"].as_str()?.to_owned()),
            name: "web_search".into(),
            arguments: value["input"].clone(),
        });
    }
    if value["type"] != "web_search_call" {
        return None;
    }
    Some(Part::ToolCall {
        presentation: sailry_protocol::tool::Presentation::Summary,
        grouping: Default::default(),
        display: display.cloned().map(Box::new),
        id: Some(value["id"].as_str()?.to_owned()),
        name: "web_search".into(),
        arguments: value.get("action").cloned().unwrap_or(Value::Null),
    })
}

pub(super) fn response(value: &Value) -> Option<Part> {
    if value["type"] == "anthropic_message" {
        // Keep the original part index without sending encrypted model context
        // or a duplicate native transcript to controller caches.
        return Some(Part::Resource(json!({"type": "provider_context"})));
    }
    if value["type"] != "web_search_tool_result" {
        return None;
    }
    let id = value["tool_use_id"].as_str()?;
    let content = value.get("content")?;
    let result = if content["type"] == "web_search_tool_result_error" {
        json!({"error": Fault::new(ErrorCode::Unavailable, "hosted web search failed")})
    } else {
        let sources: Vec<_> = content.as_array()?.iter().map(|source| {
            json!({"url": source["url"], "title": source["title"], "page_age": source["page_age"]})
        }).collect();
        json!({"sources": sources})
    };
    Some(Part::ToolResult {
        id: Some(id.into()),
        name: "web_search".into(),
        result,
        images: Vec::new(),
    })
}

/// Derived results follow the original parts to preserve approval/call indices.
pub(super) fn result(value: &Value) -> Option<Part> {
    let Part::ToolCall { id, name, .. } = call(value, None)? else {
        return None;
    };
    let result = match value["status"].as_str()? {
        "completed" => value.clone(),
        "failed" => {
            json!({"error": Fault::new(ErrorCode::Unavailable, "hosted web search failed")})
        }
        _ => return None,
    };
    Some(Part::ToolResult {
        id,
        name,
        result,
        images: Vec::new(),
    })
}
