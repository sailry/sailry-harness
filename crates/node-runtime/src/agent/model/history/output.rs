//! Reduce outgoing tool text while canonical events retain the original result.
use adk_core::{Content, Part};
use serde_json::{Value, json};
use std::collections::BTreeMap;

mod commands;
mod files;
mod git;
mod window;
pub(in crate::agent) use window::protected;
pub(in crate::agent) use window::{
    Masks, PREPARED, Prepared, clear_prepared, masked, prepare, revisions, take_prepared,
};
#[cfg(test)]
mod tests;

const LIMIT: usize = 32 * 1024;
const EDGE: usize = 16 * 1024;

fn eligible(original: &Value, name: &str, id: &str) -> bool {
    if original.get("output").is_some_and(|source| {
        source["call"].as_str() == Some(id)
            && source["revision"].as_str().is_some_and(|revision| {
                revision.len() == 64 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
    }) {
        return false;
    }
    let data = original.get("data").unwrap_or(original);
    original.get("sailry_guide_loaded").is_some()
        || original["kind"].as_str().is_some_and(|kind| {
            matches!(
                kind,
                "file_lines" | "search_results" | "command_result" | "command_output" | "web_page"
            ) || kind.starts_with("git_")
        })
        || data["lines"].is_array()
            && data["path"].is_string()
            && data["revision"].is_string()
            && data["offset"].is_u64()
        || data["matches"].is_array() && data.get("mode").is_some()
        || data["stdout"].is_object() && data["stderr"].is_object()
        || name.starts_with("mcp_")
}

fn candidate(
    original: &Value,
    name: &str,
    arguments: Option<&Value>,
    source: &Value,
    encoded: &str,
) -> Option<Value> {
    let kind = original.get("kind").and_then(Value::as_str);
    let data = original.get("data").unwrap_or(original);
    if original.get("sailry_guide_loaded").is_some_and(|guide| {
        guide["path"].is_string()
            && guide["path"] == data["path"]
            && guide["revision"] == data["revision"]
    }) {
        return Some(json!({
            "text":format!("Project guide already loaded: {}. For requested current text, use {}", data["path"].as_str().unwrap(), crate::process::output::hint(crate::process::output::ReadHint::Output)),
            "path":data["path"], "revision":data["revision"], "already_loaded":true, "output":source,
        }));
    }
    let bounded =
        || (encoded.len() > LIMIT).then(|| clip(original, source, encoded, 2 * EDGE, false));
    match kind {
        Some("file_lines") => files::lines(original, source),
        Some("search_results") => files::search(original, arguments, source),
        Some("command_result" | "command_output") => commands::project(original, source, arguments),
        Some("web_page") => bounded(),
        Some(kind) if kind.starts_with("git_") => git::project(original, source).or_else(bounded),
        None if data["lines"].is_array()
            && data["path"].is_string()
            && data["revision"].is_string()
            && data["offset"].is_u64() =>
        {
            files::lines(original, source)
        }
        None if data["matches"].is_array() && data.get("mode").is_some() => {
            files::search(original, arguments, source)
        }
        None if data["stdout"].is_object() && data["stderr"].is_object() => {
            commands::project(original, source, arguments)
        }
        _ if name.starts_with("mcp_") => bounded(),
        _ => None,
    }
}

fn command<'a>(value: &'a Value, stream: Option<&str>) -> Option<&'a str> {
    let id = crate::process::output::command_id(value)?;
    let data = value.get("data").unwrap_or(value);
    let retained = |stream: &str| {
        value["sailry_command"][stream]["retained_bytes"]
            .as_u64()
            .or_else(|| data[stream]["retained_bytes"].as_u64())
    };
    let available = match stream {
        Some(stream) => retained(stream).is_none_or(|bytes| bytes > 0),
        None => {
            let known = [retained("stdout"), retained("stderr")]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            known.is_empty() || known.into_iter().any(|bytes| bytes > 0)
        }
    };
    available.then_some(id)
}

fn hint(value: &Value, stream: Option<&str>, offset: Option<u64>) -> String {
    use crate::process::output::{ReadHint, hint};
    hint(
        command(value, stream).map_or(ReadHint::Output, |id| ReadHint::Command {
            id: Some(id),
            stream,
            offset,
        }),
    )
}

fn clip(value: &Value, source: &Value, text: &str, budget: usize, masked: bool) -> Value {
    let (head, tail) =
        crate::process::output::edges(text.as_bytes(), budget / 2, budget - budget / 2);
    let omitted = tail - head;
    let continuation = hint(value, None, None);
    let mut result = json!({
        "text":format!("{}\n[... {omitted} bytes omitted; original result retained; use {continuation} ...]\n{}", &text[..head], &text[tail..]),
        "output":source, "omitted_bytes":omitted,
    });
    if let Some(id) = command(value, None) {
        result["sailry_command"] = json!({"id":id});
    }
    if masked || value["masked"] == true {
        result["masked"] = Value::Bool(true);
    }
    for key in [
        "isError",
        "error",
        "sailry_context",
        "next_offset",
        "offset",
        "path",
        "revision",
        "partial",
        "truncated",
    ] {
        if let Some(value) = value.get(key) {
            result[key] = value.clone();
        }
    }
    result
}
