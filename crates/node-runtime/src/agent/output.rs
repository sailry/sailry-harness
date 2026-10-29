//! Tool previews restored from Sailry fb70fdc1, only in outgoing GDK messages.
use goose_provider_types::conversation::message::{Message, MessageContentBlock};
use rmcp::model::ContentBlock;
use serde_json::{Value, json};

mod commands;
mod files;
mod terminal;

pub(in crate::agent) fn project(messages: &mut [Message]) {
    for content in messages.iter_mut().flat_map(|message| &mut message.content) {
        let MessageContentBlock::ToolResponse(response) = content else {
            continue;
        };
        let Ok(result) = &mut response.tool_result else {
            continue;
        };
        let Some(original) = &result.structured_content else {
            continue;
        };
        if !matches!(
            original["kind"].as_str(),
            Some("command_result" | "command_output" | "file_lines")
        ) || original["output"]["call"] == response.id
        {
            continue;
        }
        let encoded = original.to_string();
        let source = json!({
            "call":response.id,
            "revision":blake3::hash(encoded.as_bytes()).to_hex().to_string(),
        });
        if original["kind"] == "file_lines" {
            let Some(projected) = files::project(original, &source) else {
                continue;
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
            // Plugin adapters serialize the whole result. Keep its numbered text
            // once, with the exact original JSON available through the reference.
            result.structured_content = None;
            continue;
        }
        let Some(projected) = commands::project(original, &source) else {
            continue;
        };
        let updated = projected.to_string();
        for content in &mut result.content {
            if let ContentBlock::Text(text) = content
                && text.text == encoded
            {
                text.text.clone_from(&updated);
            }
        }
        result.structured_content = Some(projected);
    }
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
