use super::tools::Raw;
use sailry_protocol::{ErrorCode, Fault};
use serde_json::{Value, json};

pub(super) fn call(value: &Value, display: Option<&sailry_protocol::tool::Display>) -> Option<Raw> {
    if value["name"] == "web_search" && value.get("input").is_some() {
        let mut raw = Raw::new(
            Some(value["id"].as_str()?.to_owned()),
            "web_search".into(),
            value["input"].clone(),
        );
        raw.display = display.cloned();
        return Some(raw);
    }
    if value["type"] != "web_search_call" {
        return None;
    }
    let mut raw = Raw::new(
        Some(value["id"].as_str()?.to_owned()),
        "web_search".into(),
        value.get("action").cloned().unwrap_or(Value::Null),
    );
    raw.display = display.cloned();
    Some(raw)
}

pub(super) fn response(value: &Value) -> Option<Raw> {
    if value["type"] == "anthropic_message" {
        // Keep the original part index without sending encrypted model context
        // or a duplicate native transcript to controller caches.
        return None;
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
    let mut raw = Raw::new(Some(id.into()), "web_search".into(), Value::Null);
    raw.result = Some(result);
    Some(raw)
}

/// Derived results follow the original parts to preserve approval/call indices.
pub(super) fn result(
    value: &Value,
    display: Option<&sailry_protocol::tool::Display>,
) -> Option<Raw> {
    let mut raw = call(value, display)?;
    let result = match value["status"].as_str()? {
        "completed" => value.clone(),
        "failed" => {
            json!({"error": Fault::new(ErrorCode::Unavailable, "hosted web search failed")})
        }
        _ => return None,
    };
    raw.result = Some(result);
    Some(raw)
}
