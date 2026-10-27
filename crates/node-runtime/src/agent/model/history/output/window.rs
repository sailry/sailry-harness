//! Request-only budgets retain canonical results and their continuation references.
use super::*;
use crate::agent::{compaction::estimate, model::history::Window};
use adk_core::LlmRequest;
use std::collections::BTreeSet;

/// Stored result revisions by call; references must match the canonical read path.
pub(in crate::agent) type Revisions = BTreeMap<String, BTreeSet<String>>;
pub(in crate::agent) type Masks = BTreeSet<(String, String)>;

pub(in crate::agent) fn protected(name: &str, value: &Value) -> bool {
    matches!(name, "load_skill" | "read_skill_resource" | "plan_review")
        || value.get("progress").is_some_and(|progress| {
            serde_json::from_value::<sailry_protocol::conversation::progress::Progress>(
                progress.clone(),
            )
            .is_ok()
        })
}

pub(in crate::agent) fn revisions(events: &[adk_core::Event]) -> Revisions {
    collect(events.iter().filter_map(adk_core::Event::content))
}

pub(in crate::agent) fn references(contents: &[Content]) -> Revisions {
    collect(contents.iter())
}

fn collect<'a>(contents: impl Iterator<Item = &'a Content>) -> Revisions {
    contents
        .flat_map(|content| &content.parts)
        .filter_map(|part| match part {
            Part::FunctionResponse {
                id: Some(id),
                function_response,
                ..
            } if !id.is_empty() => Some((id.clone(), hash(&function_response.response))),
            _ => None,
        })
        .fold(Revisions::new(), |mut revisions, (id, revision)| {
            revisions.entry(id).or_default().insert(revision);
            revisions
        })
}

fn hash(value: &Value) -> String {
    blake3::hash(value.to_string().as_bytes())
        .to_hex()
        .to_string()
}

pub(super) fn source(value: &Value, id: &str, revisions: &Revisions) -> Value {
    if let Some(source) = value.get("output")
        && source["call"].as_str() == Some(id)
        && source["revision"].as_str().is_some_and(|revision| {
            revision.len() == 64 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return source.clone();
    }
    let current = hash(value);
    let revision = revisions
        .get(id)
        .filter(|known| !known.contains(&current) && known.len() == 1)
        .and_then(|known| known.first())
        .unwrap_or(&current);
    json!({"call":id,"revision":revision})
}

fn excerpt(
    value: &Value,
    id: &str,
    characters: usize,
    masked: bool,
    revisions: &Revisions,
) -> Value {
    let text = value.to_string();
    let count = text.chars().count();
    let head = text
        .char_indices()
        .nth(characters / 2)
        .map_or(text.len(), |(index, _)| index);
    let tail = text
        .chars()
        .rev()
        .take(characters - characters / 2)
        .map(char::len_utf8)
        .sum();
    let (head, tail) = crate::process::output::edges(text.as_bytes(), head, tail);
    let (head, tail) = (&text[..head], &text[tail..]);
    let command = super::commands::id(value);
    let continuation = command.map_or_else(
        || "read_tool_output with output, offset and limit".into(),
        |id| format!("read_command id={id} with stream, offset and limit; use read_tool_output with output, offset and limit for the persisted result JSON"),
    );
    let mut result = json!({
        "text":format!("{head}\n[... original result retained; use {continuation} ...]\n{tail}"),
        "output":source(value,id,revisions), "omitted_characters":count.saturating_sub(characters),
    });
    if let Some(id) = command {
        result["sailry_command"] = json!({"id":id});
    }
    if masked {
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

fn maskable(id: &str, name: &str, value: &Value) -> bool {
    !id.is_empty() && !protected(name, value) && value.get("masked") != Some(&Value::Bool(true))
}

/// Protect the newest output-token window. Commit the projection only when the
/// entire older-result pass reaches the requested minimum saving.
pub(in crate::agent) fn mask(
    request: &mut LlmRequest,
    protect: u64,
    minimum: u64,
    revisions: &Revisions,
) -> adk_core::Result<u64> {
    let mut recent = 0u64;
    let mut saved = 0u64;
    let mut replacements = Vec::new();
    for (content_index, content) in request.contents.iter().enumerate().rev() {
        for (part_index, part) in content.parts.iter().enumerate().rev() {
            let Part::FunctionResponse {
                id: Some(id),
                function_response,
                ..
            } = part
            else {
                continue;
            };
            let value = &function_response.response;
            let tokens = estimate::text(&value.to_string());
            let keep = recent < protect;
            recent = recent.saturating_add(tokens);
            if keep || !maskable(id, &function_response.name, value) {
                continue;
            }
            let replacement = excerpt(value, id, 500, true, revisions);
            let reduction = tokens.saturating_sub(estimate::text(&replacement.to_string()));
            if reduction > 0 {
                saved = saved.saturating_add(reduction);
                replacements.push((content_index, part_index, replacement));
            }
        }
    }
    if saved < minimum {
        return Ok(0);
    }
    for (content, part, replacement) in replacements {
        let Part::FunctionResponse {
            function_response, ..
        } = &mut request.contents[content].parts[part]
        else {
            unreachable!("the request is unchanged while selecting result projections");
        };
        function_response.response = replacement;
    }
    if saved > 0 {
        request.previous_response_id = None;
    }
    Ok(saved)
}

pub(in crate::agent) fn masked(request: &LlmRequest) -> Masks {
    request
        .contents
        .iter()
        .flat_map(|content| &content.parts)
        .filter_map(|part| match part {
            Part::FunctionResponse {
                id: Some(id),
                function_response,
                ..
            } if function_response.response.get("masked") == Some(&Value::Bool(true)) => {
                function_response.response["output"]["revision"]
                    .as_str()
                    .map(|revision| (id.clone(), revision.into()))
            }
            _ => None,
        })
        .collect()
}

/// Repeat earlier masks so one batch projection keeps a stable request prefix.
pub(in crate::agent) fn remask(request: &mut LlmRequest, calls: &Masks, revisions: &Revisions) {
    if calls.is_empty() {
        return;
    }
    for part in request
        .contents
        .iter_mut()
        .flat_map(|content| &mut content.parts)
    {
        if let Part::FunctionResponse {
            id: Some(id),
            function_response,
            ..
        } = part
            && calls.contains(&(
                id.clone(),
                source(&function_response.response, id, revisions)["revision"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            ))
            && maskable(id, &function_response.name, &function_response.response)
        {
            function_response.response =
                excerpt(&function_response.response, id, 500, true, revisions);
            request.previous_response_id = None;
        }
    }
}

/// Bound ordinary results by the remaining captured window, including results
/// from MCP and delegated tools. Immutable inputs are never shortened. Returns
/// a fingerprint of the shortened results, if any.
pub(in crate::agent) fn limit(
    request: &mut LlmRequest,
    window: Option<Window>,
    revisions: &Revisions,
) -> adk_core::Result<Option<String>> {
    let Some(window) = window else {
        return Ok(None);
    };
    let output = request
        .config
        .as_ref()
        .and_then(|config| config.max_output_tokens)
        .map_or(window.output, |output| output.max(0) as u64)
        .min(window.output);
    let capacity = window.context.saturating_sub(output);
    let total = estimate::request(request)?;
    let mut fingerprint = None;
    if total > capacity {
        let mut sizes = Vec::new();
        for part in request.contents.iter().flat_map(|content| &content.parts) {
            if let Part::FunctionResponse {
                id: Some(id),
                function_response,
                ..
            } = part
                && !id.is_empty()
                && !protected(&function_response.name, &function_response.response)
            {
                sizes.push(estimate::text(&function_response.response.to_string()));
            }
        }
        let results = sizes.iter().sum::<u64>();
        let mut available = capacity.saturating_sub(total.saturating_sub(results));
        // Results that fit an equal share stay intact; larger ones share the rest.
        sizes.sort_unstable();
        let mut share = 0;
        for (index, size) in sizes.iter().enumerate() {
            share = available / (sizes.len() - index) as u64;
            if *size > share {
                break;
            }
            available -= size;
        }
        let characters = (share.saturating_sub(128) / 2) as usize;
        let mut replaced = blake3::Hasher::new();
        let mut changed = false;
        for part in request
            .contents
            .iter_mut()
            .flat_map(|content| &mut content.parts)
        {
            let Part::FunctionResponse {
                id: Some(id),
                function_response,
                ..
            } = part
            else {
                continue;
            };
            if id.is_empty() || protected(&function_response.name, &function_response.response) {
                continue;
            }
            let value = &function_response.response;
            let tokens = estimate::text(&value.to_string());
            if tokens > share {
                let replacement = excerpt(value, id, characters, false, revisions);
                if estimate::text(&replacement.to_string()) < tokens {
                    replaced.update(id.as_bytes());
                    replaced.update(replacement.to_string().as_bytes());
                    function_response.response = replacement;
                    request.previous_response_id = None;
                    changed = true;
                }
            }
        }
        fingerprint = changed.then(|| replaced.finalize().to_hex().to_string());
    }
    if estimate::request(request)? > capacity {
        return Err(adk_core::AdkError::new(
            adk_core::ErrorComponent::Model,
            adk_core::ErrorCategory::ResourceExhausted,
            "model.sailry.context_overflow",
            "retained context does not fit the captured model window",
        ));
    }
    Ok(fingerprint)
}

#[cfg(test)]
mod tests;
