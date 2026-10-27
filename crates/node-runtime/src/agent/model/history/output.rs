//! Reduce outgoing tool text while canonical events retain the original result.
use adk_core::{Content, Part};
use serde_json::{Value, json};
use std::collections::BTreeMap;

mod commands;
mod files;
mod git;
mod window;
pub(in crate::agent) use window::{Masks, Revisions, masked, remask, revisions};
pub(super) use window::{limit, mask, protected, references};
#[cfg(test)]
mod tests;

const LIMIT: usize = 32 * 1024;
const EDGE: usize = 16 * 1024;

#[cfg(test)]
fn project(contents: &mut [Content]) {
    project_with(contents, &Revisions::new());
}

pub(super) fn project_with(contents: &mut [Content], revisions: &Revisions) {
    let mut calls = BTreeMap::new();
    for part in contents.iter_mut().flat_map(|content| &mut content.parts) {
        if let Part::FunctionCall {
            id: Some(id),
            name,
            args,
            ..
        } = part
        {
            calls.insert(id.clone(), (name.clone(), args.clone()));
            continue;
        }
        let Part::FunctionResponse {
            function_response,
            id: Some(id),
            ..
        } = part
        else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        let original = &function_response.response;
        if original.get("output").is_some_and(|source| {
            source["call"].as_str() == Some(id.as_str())
                && source["revision"].as_str().is_some_and(|revision| {
                    revision.len() == 64 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
        }) {
            continue;
        }
        let source = window::source(original, id, revisions);
        let kind = original.get("kind").and_then(Value::as_str);
        let data = original.get("data").unwrap_or(original);
        let candidate = if original.get("sailry_guide_loaded").is_some_and(|guide| {
            guide["path"].is_string()
                && guide["path"] == data["path"]
                && guide["revision"] == data["revision"]
        }) {
            Some(json!({
                "text":format!("Project guide already loaded: {}. For requested current text, use read_tool_output with output, offset and limit", data["path"].as_str().unwrap()),
                "path":data["path"], "revision":data["revision"],
                "already_loaded":true, "output":source,
            }))
        } else {
            match kind {
                Some("file_lines") => files::lines(original, &source),
                Some("search_results") => files::search(
                    original,
                    calls
                        .get(id)
                        .filter(|(name, _)| name == &function_response.name)
                        .map(|(_, arguments)| arguments),
                    &source,
                ),
                Some("command_result" | "command_output") => commands::project(
                    original,
                    &source,
                    calls.get(id).map(|(_, arguments)| arguments),
                ),
                Some("web_page") => bounded(original, &source),
                Some(kind) if kind.starts_with("git_") => {
                    git::project(original, &source).or_else(|| bounded(original, &source))
                }
                // Script-backed host operations return their validated data
                // directly, while native operations retain the Output envelope.
                None if data.get("lines").is_some_and(Value::is_array)
                    && data["path"].is_string()
                    && data["revision"].is_string()
                    && data["offset"].is_u64() =>
                {
                    files::lines(original, &source)
                }
                None if data.get("matches").is_some_and(Value::is_array)
                    && data.get("mode").is_some() =>
                {
                    files::search(
                        original,
                        calls
                            .get(id)
                            .filter(|(name, _)| name == &function_response.name)
                            .map(|(_, arguments)| arguments),
                        &source,
                    )
                }
                None if data["stdout"].is_object() && data["stderr"].is_object() => {
                    commands::project(
                        original,
                        &source,
                        calls.get(id).map(|(_, arguments)| arguments),
                    )
                }
                _ if function_response.name.starts_with("mcp_") => bounded(original, &source),
                _ => None,
            }
        };
        if let Some(mut candidate) = candidate {
            if let Some(context) = original.get("sailry_context") {
                candidate["sailry_context"] = context.clone();
            }
            if candidate.to_string().len() < original.to_string().len() {
                function_response.response = candidate;
            }
        }
    }
}

fn bounded(value: &Value, source: &Value) -> Option<Value> {
    let text = value.to_string();
    if text.len() <= LIMIT {
        return None;
    }
    let (head, tail) = crate::process::output::edges(text.as_bytes(), EDGE, EDGE);
    let (head, tail) = (&text[..head], &text[tail..]);
    let omitted = text.len() - head.len() - tail.len();
    let mut result = json!({
        "text":format!("{head}\n[... {omitted} bytes omitted; use read_tool_output with output, offset and limit for the original result ...]\n{tail}"),
        "output":source,
        "omitted_bytes":omitted
    });
    for key in ["isError", "error"] {
        if let Some(value) = value.get(key) {
            result[key] = value.clone();
        }
    }
    Some(result)
}
