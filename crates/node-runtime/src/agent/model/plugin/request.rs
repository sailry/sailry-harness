//! ADK remains the history and execution owner; translations receive neutral data.
use adk_core::{LlmRequest, Part};
use base64::Engine;
use sailry_protocol::{
    Effort, Fault,
    plugin::model_adapter::{Capabilities, wire},
};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn convert(
    request: LlmRequest,
    endpoint: &str,
    session: &str,
    effort: Effort,
    features: BTreeMap<String, String>,
    capabilities: &Capabilities,
    stream: bool,
) -> Result<wire::Request, Fault> {
    if !capabilities.reasoning && !matches!(effort, Effort::Default | Effort::Disabled) {
        return Err(unsupported());
    }
    let config = request.config.unwrap_or_default();
    if config.temperature.is_some_and(|value| !value.is_finite())
        || config.top_p.is_some_and(|value| !value.is_finite())
    {
        return Err(invalid());
    }
    let mut options: BTreeMap<String, Value> = config.extensions.clone().into_iter().collect();
    for (name, value) in [
        ("top_k", serde_json::to_value(config.top_k)),
        (
            "frequency_penalty",
            serde_json::to_value(config.frequency_penalty),
        ),
        (
            "presence_penalty",
            serde_json::to_value(config.presence_penalty),
        ),
        ("seed", serde_json::to_value(config.seed)),
        ("top_logprobs", serde_json::to_value(config.top_logprobs)),
        (
            "response_schema",
            serde_json::to_value(&config.response_schema),
        ),
        (
            "cached_content",
            serde_json::to_value(&config.cached_content),
        ),
        (
            "previous_response_id",
            serde_json::to_value(request.previous_response_id),
        ),
    ] {
        let value = value.map_err(|_| invalid())?;
        if !value.is_null() {
            options.insert(name.into(), value);
        }
    }
    if !request.tools.is_empty() && !capabilities.tools {
        return Err(unsupported());
    }
    let mut tools: Vec<_> = request
        .tools
        .into_iter()
        .map(|(name, declaration)| wire::Tool {
            name,
            description: declaration
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned),
            parameters: declaration
                .get("parameters")
                .cloned()
                .unwrap_or(declaration),
        })
        .collect();
    tools.sort_by(|left, right| left.name.cmp(&right.name));
    let mut messages = Vec::new();
    let mut system = Vec::new();
    for content in request.contents {
        if matches!(content.role.as_str(), "system" | "developer") {
            for part in content.parts {
                match part {
                    Part::Text { text } => system.push(text),
                    _ => return Err(unsupported()),
                }
            }
            continue;
        }
        let mut pending = Vec::new();
        let assistant = matches!(content.role.as_str(), "model" | "assistant");
        if !assistant && !matches!(content.role.as_str(), "user" | "function" | "tool") {
            return Err(unsupported());
        }
        for part in content.parts {
            if let Part::FunctionResponse {
                function_response,
                id,
                ..
            } = part
            {
                flush(&mut messages, &mut pending, assistant);
                let mut parts = vec![wire::Part::Json {
                    value: function_response.response,
                }];
                for part in function_response.inline_data {
                    parts.push(binary(part.mime_type, part.data, capabilities)?);
                }
                for part in function_response.file_data {
                    parts.push(reference(part.mime_type, part.file_uri, capabilities)?);
                }
                messages.push(wire::Message::Tool {
                    call_id: id.filter(|id| !id.is_empty()).ok_or_else(unsupported)?,
                    name: function_response.name,
                    parts,
                    is_error: false,
                });
            } else {
                pending.push(convert_part(part, capabilities)?);
            }
        }
        flush(&mut messages, &mut pending, assistant);
    }
    let max_output_tokens = config
        .max_output_tokens
        .map(|value| u64::try_from(value).map_err(|_| invalid()))
        .transpose()?;
    Ok(wire::Request {
        version: "v1".into(),
        model: request.model,
        messages,
        system: (!system.is_empty()).then(|| system.join("\n\n")),
        tools,
        tool_choice: wire::Choice::Auto,
        reasoning: wire::Reasoning {
            effort,
            replay: capabilities.reasoning,
        },
        features,
        max_output_tokens,
        temperature: config.temperature.map(f64::from),
        top_p: config.top_p.map(f64::from),
        stop: config.stop_sequences,
        stream,
        session: session.into(),
        endpoint: endpoint.into(),
        options,
    })
}

fn flush(messages: &mut Vec<wire::Message>, parts: &mut Vec<wire::Part>, assistant: bool) {
    if parts.is_empty() {
        return;
    }
    let parts = std::mem::take(parts);
    messages.push(if assistant {
        wire::Message::Assistant { parts }
    } else {
        wire::Message::User { parts }
    });
}

fn convert_part(value: Part, capabilities: &Capabilities) -> Result<wire::Part, Fault> {
    Ok(match value {
        Part::Text { text } => wire::Part::Text { text },
        Part::Thinking {
            thinking,
            signature,
        } if capabilities.reasoning => wire::Part::Reasoning {
            text: Some(thinking),
            replay: signature,
        },
        Part::FunctionCall {
            name,
            args,
            id,
            thought_signature,
        } if capabilities.tools => {
            // A signed call cannot be represented as an unsigned call. Native
            // providers continue to own their provider-specific signed history.
            if thought_signature.is_some() {
                return Err(unsupported());
            }
            wire::Part::ToolCall {
                name,
                arguments: args,
                id: id.filter(|id| !id.is_empty()).ok_or_else(unsupported)?,
            }
        }
        Part::InlineData {
            mime_type, data, ..
        } => binary(mime_type, data, capabilities)?,
        Part::FileData {
            mime_type,
            file_uri,
            ..
        } => reference(mime_type, file_uri, capabilities)?,
        Part::EmbeddedResource {
            resource: adk_core::EmbeddedResource::Text(value),
        } => wire::Part::Text { text: value.text },
        Part::EmbeddedResource {
            resource: adk_core::EmbeddedResource::Blob(value),
        } => binary(
            value
                .mime_type
                .unwrap_or_else(|| "application/octet-stream".into()),
            value.data,
            capabilities,
        )?,
        _ => return Err(unsupported()),
    })
}

fn binary(
    media_type: String,
    data: Vec<u8>,
    capabilities: &Capabilities,
) -> Result<wire::Part, Fault> {
    let data_base64 = base64::engine::general_purpose::STANDARD.encode(data);
    if media_type.starts_with("image/") && capabilities.vision {
        Ok(wire::Part::Image {
            media_type,
            data: wire::Image::Inline { data_base64 },
        })
    } else if capabilities.files {
        Ok(wire::Part::File {
            media_type,
            name: None,
            data_base64,
        })
    } else {
        Err(unsupported())
    }
}

fn reference(
    media_type: String,
    url: String,
    capabilities: &Capabilities,
) -> Result<wire::Part, Fault> {
    if media_type.starts_with("image/") && capabilities.vision {
        Ok(wire::Part::Image {
            media_type,
            data: wire::Image::Url { url },
        })
    } else {
        Err(unsupported())
    }
}

fn invalid() -> Fault {
    Fault::invalid("model input has an invalid generation value")
}
fn unsupported() -> Fault {
    Fault::invalid("model translation does not support this input part")
}
