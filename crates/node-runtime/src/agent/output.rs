//! Tool previews restored from Sailry fb70fdc1, only in outgoing model messages.
use crate::agent::model::{Content, Message, types::ToolResult};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{Value, json};

mod cache;
mod changes;
mod commands;
mod edits;
mod files;
mod limit;
mod pages;
mod search;
mod terminal;

pub(in crate::agent) use cache::Cache;

/// Model-facing line ceiling for one tool result, as in OpenCode 3ba597f
/// `tool-output.ts` and Pi 90b42bc `truncate.ts`. The canonical result keeps
/// every line and stays readable through `read_tool_output`.
pub(super) const MAX_LINES: usize = 2_000;
/// Model-facing UTF-8 byte ceiling for one tool result; see [`MAX_LINES`].
pub(super) const MAX_BYTES: usize = 50 * 1024;

#[cfg(test)]
pub(in crate::agent) fn project(messages: &mut [Message]) {
    // Only results still visible to the model, such as those after the latest
    // compaction boundary, can stand in for a later unchanged read.
    let mut reads = files::Reads::default();
    for message in messages {
        let visible = message.is_agent_visible();
        for content in &mut message.content {
            let Content::ToolResult(response) = content else {
                continue;
            };
            let revision = message.metadata.operation_note("tool_output", &response.id);
            response_projection(response, revision, visible, &mut reads);
        }
    }
}

fn response_projection(
    response: &mut ToolResult,
    revision: Option<&Value>,
    visible: bool,
    reads: &mut files::Reads,
) {
    let result = match &mut response.result {
        Ok(result) => result,
        Err(error) => {
            limit::error(&response.id, error, revision);
            return;
        }
    };
    let source = limit::reference(&response.id, result, revision);
    let tail = result.structured_content.as_ref().is_some_and(|value| {
        matches!(
            value["kind"].as_str(),
            Some("command_result" | "command_output")
        )
    });
    preview(&response.id, result, &source, visible.then_some(reads));
    limit::bound(result, &source, tail);
}

fn preview(
    call: &str,
    result: &mut CallToolResult,
    source: &Value,
    reads: Option<&mut files::Reads>,
) {
    let Some(original) = &result.structured_content else {
        return;
    };
    if original["output"]["call"] == call {
        return;
    }
    let encoded = original.to_string();
    let page = pages::project(original);
    let edited = original["kind"] == "file_written" && !original["data"]["excerpt"].is_null();
    if page.is_some()
        || edited
        || matches!(
            original["kind"].as_str(),
            Some("file_lines" | "search_results" | "file_changes")
        )
    {
        let projected = if page.is_some() {
            page
        } else if edited {
            edits::project(original)
        } else if original["kind"] == "file_changes" {
            changes::project(original)
        } else if original["kind"] == "file_lines" {
            files::project(original, source, reads)
        } else {
            search::project(original, source)
        };
        let Some(projected) = projected else {
            return;
        };
        let mut mirrored = false;
        for content in &mut result.content {
            if let ContentBlock::Text(text) = content
                && (text.text == encoded
                    || serde_json::from_str::<Value>(&text.text)
                        .is_ok_and(|value| &value == original))
            {
                text.text.clone_from(&projected);
                mirrored = true;
            }
        }
        if !mirrored {
            result.content.push(ContentBlock::text(projected));
        }
        result.structured_content = None;
        return;
    }
    if !matches!(
        original["kind"].as_str(),
        Some("command_result" | "command_output")
    ) {
        return;
    }
    let Some(projected) = commands::project(original, source) else {
        return;
    };
    let updated = projected.to_string();
    for content in &mut result.content {
        if let ContentBlock::Text(text) = content
            && (text.text == encoded
                || serde_json::from_str::<Value>(&text.text).is_ok_and(|value| &value == original))
        {
            text.text.clone_from(&updated);
        }
    }
    result.structured_content = Some(projected);
}

fn hint(value: &Value, stream: Option<&str>, offset: Option<u64>) -> String {
    let output = "read_tool_output with output, offset and limit for the persisted result JSON";
    let Some(id) = commands::id(value) else {
        return output.into();
    };
    let data = value.get("data").unwrap_or(value);
    if stream.is_some_and(|stream| {
        value["sailry_command"][stream]["retained_bytes"]
            .as_u64()
            .or_else(|| data[stream]["retained_bytes"].as_u64())
            == Some(0)
    }) {
        return output.into();
    }
    let mut command = format!("read_command id={id}");
    if let Some(stream) = stream {
        command.push_str(&format!(" stream={stream}"));
    }
    if let Some(offset) = offset {
        command.push_str(&format!(" offset={offset}"));
    }
    command.push_str(" with limit");
    if stream.is_none() {
        command.push_str(" and stream");
    }
    if offset.is_none() {
        command.push_str(" and offset");
    }
    format!("{command}; use {output} if retained output is unavailable")
}

#[cfg(test)]
mod tests;
