//! Pi-compatible request identity and tool naming for Claude subscription grants.
//! Adapted from Pi 4270522473e677ddba7b093caa0064a3df51d459.
//! Copyright (c) 2025 Mario Zechner. MIT license: third_party_licenses/pi-MIT.txt.
use super::*;
use futures::StreamExt;

const IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";
const TOOLS: &[&str] = &[
    "Read",
    "Write",
    "Edit",
    "Bash",
    "Grep",
    "Glob",
    "AskUserQuestion",
    "EnterPlanMode",
    "ExitPlanMode",
    "KillShell",
    "NotebookEdit",
    "Skill",
    "Task",
    "TaskOutput",
    "TodoWrite",
    "WebFetch",
    "WebSearch",
];

pub(super) fn adapt(
    body: &mut Value,
    headers: &mut HeaderMap,
    authorized: &HeaderMap,
) -> Result<(), AdkError> {
    // Keep SDK-generated beta flags when adding OAuth identity headers.
    let mut features = headers
        .get("anthropic-beta")
        .map(|value| value.to_str())
        .transpose()
        .map_err(|_| invalid())?
        .unwrap_or_default()
        .split(',')
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_owned())
        .collect::<Vec<_>>();
    headers.extend(authorized.clone());
    headers.remove("x-api-key");
    if let Some(value) = headers.get("anthropic-beta") {
        for feature in value.to_str().map_err(|_| invalid())?.split(',') {
            if !features.iter().any(|value| value == feature.trim()) {
                features.push(feature.trim().to_owned());
            }
        }
    }

    let fields = body.as_object_mut().ok_or_else(invalid)?;
    let mut system = vec![json!({
        "type": "text", "text": IDENTITY,
        "cache_control": {"type": "ephemeral"},
    })];
    match fields.remove("system") {
        Some(Value::String(text)) if !text.is_empty() => system.push(json!({
            "type": "text", "text": text,
            "cache_control": {"type": "ephemeral"},
        })),
        Some(Value::Array(blocks)) => system.extend(blocks),
        Some(Value::Null) | None | Some(Value::String(_)) => {}
        _ => return Err(invalid()),
    }
    fields.insert("system".into(), Value::Array(system));
    if let Some(tools) = fields.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools.iter_mut() {
            rename(tool)?;
        }
        if !tools.is_empty() {
            // The pinned SDK lacks eager_input_streaming; Pi uses this beta
            // when that tool field is unavailable.
            features.push("fine-grained-tool-streaming-2025-05-14".into());
        }
    }
    if fields
        .get("thinking")
        .is_some_and(|thinking| thinking["type"] == "enabled")
    {
        features.push("interleaved-thinking-2025-05-14".into());
    }
    if let Some(choice) = fields.get_mut("tool_choice")
        && choice["type"] == "tool"
    {
        rename(choice)?;
    }
    if let Some(messages) = fields.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages {
            if let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) {
                for block in content {
                    if block["type"] == "tool_use" {
                        rename(block)?;
                    }
                }
            }
        }
    }
    features.sort();
    features.dedup();
    headers.insert(
        "anthropic-beta",
        HeaderValue::from_str(&features.join(",")).map_err(|_| invalid())?,
    );
    Ok(())
}

fn rename(block: &mut Value) -> Result<(), AdkError> {
    let name = block
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    if let Some(canonical) = TOOLS.iter().find(|tool| tool.eq_ignore_ascii_case(name)) {
        block["name"] = json!(canonical);
    }
    Ok(())
}

pub(super) fn restore(stream: LlmResponseStream, tools: Vec<String>) -> LlmResponseStream {
    Box::pin(stream.map(move |response| {
        response.map(|mut response| {
            if let Some(content) = &mut response.content {
                for part in &mut content.parts {
                    if let Part::FunctionCall { name, .. } = part
                        && let Some(original) =
                            tools.iter().find(|tool| tool.eq_ignore_ascii_case(name))
                    {
                        *name = original.clone();
                    }
                }
            }
            response
        })
    }))
}

#[cfg(test)]
mod tests;
