//! Canonical JSON pages remain readable without another layer of JSON escaping.
use super::*;
use sailry_protocol::tool::OutputPage;

pub(super) fn project(value: &Value) -> Option<String> {
    let fields = value.as_object()?;
    if fields.len() != 6 || value.get("kind").is_some() {
        return None;
    }
    let page: OutputPage = serde_json::from_value(value.clone()).ok()?;
    Some(format!(
        "Result bytes {}–{} of {}\n{}\n{}",
        page.offset,
        page.offset.checked_add(page.text.len())?,
        page.total_bytes,
        page.text,
        json!({"output":page.output,"next_offset":page.next_offset,
            "read":"read_tool_output with the same output and next_offset"}),
    ))
}
