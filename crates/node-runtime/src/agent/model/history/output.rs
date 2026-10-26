//! Reduce outgoing tool text while canonical events retain the original result.
use adk_core::{Content, Part};
use serde_json::{Value, json};
use std::collections::BTreeMap;

mod commands;
mod files;
mod git;
#[cfg(test)]
mod tests;

const LIMIT: usize = 32 * 1024;
const EDGE: usize = 16 * 1024;

pub(super) fn project(contents: &mut [Content]) {
    project_with(contents);
}

fn project_with(contents: &mut [Content]) {
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
        let source = json!({"call":id,"revision":blake3::hash(original.to_string().as_bytes()).to_hex().to_string()});
        let kind = original.get("kind").and_then(Value::as_str);
        let data = original.get("data").unwrap_or(original);
        let candidate = if original.get("sailry_guide_loaded").is_some_and(|guide| {
                guide["path"].is_string()
                    && guide["path"] == data["path"]
                    && guide["revision"] == data["revision"]
            }) {
            Some(json!({
                "text":format!("Project guide already loaded: {}. For requested current text, use read_command with output, offset and limit", data["path"].as_str().unwrap()),
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
    let head = floor(&text, EDGE);
    let tail = ceil(&text, text.len() - EDGE);
    let omitted = tail - head;
    let mut result = json!({
        "text":format!("{}\n[... {omitted} bytes omitted; use read_command with output, offset and limit for the original result ...]\n{}", &text[..head], &text[tail..]),
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

fn floor(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn ceil(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}
