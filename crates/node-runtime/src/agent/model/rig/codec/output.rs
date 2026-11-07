use super::types;
use base64::Engine as _;
use rig_core::message::{AssistantContent, DocumentSourceKind, MimeType};
use serde_json::{Map, Value};
use types::{Content, Error, ErrorKind, Image, ToolCall, ToolError};

pub(super) fn content(source: AssistantContent) -> Result<Content, Error> {
    let native = source.native_item().cloned();
    Ok(match source {
        AssistantContent::Text(text) => Content::Text { text: text.text },
        AssistantContent::Reasoning(reasoning) => {
            if reasoning.redacted {
                let data = native
                    .as_ref()
                    .filter(|item| {
                        item.get("type").and_then(Value::as_str) == Some("redacted_thinking")
                    })
                    .and_then(|item| item.get("data"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("redacted reasoning payload is unavailable"))?;
                Content::RedactedThinking { data: data.into() }
            } else {
                let signature = native
                    .as_ref()
                    .and_then(|item| {
                        item.get("signature")
                            .or_else(|| item.get("thoughtSignature"))
                    })
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);
                Content::Thinking {
                    text: reasoning.text,
                    signature,
                }
            }
        }
        AssistantContent::ToolCall(call) => {
            let error = native
                .as_ref()
                .is_some_and(|item| item[super::super::anthropic::INCOMPLETE] == true)
                .then(|| ToolError {
                    code: -32602,
                    message: "tool call ended before its content block was complete".into(),
                    data: None,
                })
                .or_else(|| {
                    call.function
                        .invalid_arguments
                        .as_ref()
                        .map(|arguments| ToolError {
                            code: -32602,
                            message: "tool arguments must be a JSON object".into(),
                            data: Some(Value::String(arguments.clone())),
                        })
                });
            Content::ToolCall(ToolCall {
                id: call.id.to_string(),
                name: call.function.name.to_string(),
                arguments: Value::Object(call.function.arguments),
                metadata: metadata(native),
                tool_meta: None,
                error,
            })
        }
        AssistantContent::Image(image) => {
            let data = match image.data {
                DocumentSourceKind::Base64(data_base64) => Image::Inline { data_base64 },
                DocumentSourceKind::Url(url) => Image::Url { url },
                DocumentSourceKind::Raw(bytes) => Image::Inline {
                    data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                },
                _ => return Err(invalid("provider image source is unavailable")),
            };
            Content::Image {
                media_type: image
                    .media_type
                    .as_ref()
                    .ok_or_else(|| invalid("provider image media type is unavailable"))?
                    .to_mime_type()
                    .into(),
                data,
                annotations: None,
            }
        }
        _ => {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "provider returned unsupported assistant content",
            ));
        }
    })
}

fn metadata(native: Option<Value>) -> Option<Map<String, Value>> {
    let mut fields = native?.as_object()?.clone();
    let standard: &[&str] = if fields.contains_key("functionCall") {
        &["functionCall"]
    } else if fields.contains_key("function") {
        &["id", "function", "type", "index"]
    } else if fields.get("type").and_then(Value::as_str) == Some("tool_use") {
        &["type", "id", "name", "input"]
    } else {
        &["type", "id", "call_id", "name", "arguments", "status"]
    };
    for key in standard {
        fields.remove(*key);
    }
    fields.remove(super::super::anthropic::INCOMPLETE);
    (!fields.is_empty()).then_some(fields)
}

fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::InvalidResponse, message)
}
