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
                            .or_else(|| item.get("encrypted_content"))
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

pub(super) fn partial_usage(
    source: &rig_core::completion::CompletionResponse,
    mut usage: types::Usage,
) -> types::Usage {
    let Some(raw) = source
        .raw
        .get("usage")
        .or_else(|| source.raw.get("usageMetadata"))
        .or_else(|| source.raw.pointer("/choices/0/usage"))
    else {
        return usage;
    };
    let reported = match source.origin.api.as_str() {
        "openai.chat" => return chat_usage(raw, usage),
        "gemini.generate_content" => rig_core::providers::gemini::completion::usage_of(raw),
        "anthropic.messages" => {
            use rig_core::{completion::Usage, json_utils::Lenient};

            let count = |pointer| raw.pointer(pointer).and_then(Lenient::as_u64_lenient);
            let cached_input_tokens = count("/cache_read_input_tokens");
            let cache_creation_input_tokens = count("/cache_creation_input_tokens");
            let input_tokens = count("/input_tokens").map(|input| {
                input
                    .saturating_add(cached_input_tokens.unwrap_or(0))
                    .saturating_add(cache_creation_input_tokens.unwrap_or(0))
            });
            let output_tokens = count("/output_tokens");
            let mut reported = Usage::default();
            reported.input_tokens = input_tokens;
            reported.output_tokens = output_tokens;
            reported.total_tokens = input_tokens
                .zip(output_tokens)
                .map(|(input, output)| input.saturating_add(output));
            reported.cached_input_tokens = cached_input_tokens;
            reported.cache_creation_input_tokens = cache_creation_input_tokens;
            reported.reasoning_tokens = count("/output_tokens_details/thinking_tokens");
            reported
        }
        _ => return usage,
    };
    usage.input = usage.input.or(reported.input_tokens);
    usage.output = usage.output.or(reported.output_tokens);
    usage.total = usage.total.or(reported.total_tokens);
    usage.cache_read = usage.cache_read.or(reported.cached_input_tokens);
    usage.cache_write = usage.cache_write.or(reported.cache_creation_input_tokens);
    usage.reasoning = usage.reasoning.or(reported.reasoning_tokens);
    usage.provider.get_or_insert_with(|| raw.clone());
    usage
}

fn chat_usage(raw: &Value, mut usage: types::Usage) -> types::Usage {
    let count = |pointer| raw.pointer(pointer).and_then(Value::as_u64);
    usage.total = usage.total.or_else(|| count("/total_tokens"));
    usage.input = usage.input.or_else(|| {
        count("/prompt_tokens").map(|input| {
            let audio = count("/prompt_tokens_details/audio_tokens").unwrap_or(0);
            let beside = input.saturating_add(audio);
            if audio != 0
                && Some(beside.saturating_add(count("/completion_tokens").unwrap_or(0)))
                    == usage.total
            {
                beside
            } else {
                input
            }
        })
    });
    usage.output = usage.output.or_else(|| {
        count("/completion_tokens").or_else(|| {
            usage
                .total
                .map(|total| total.saturating_sub(usage.input.unwrap_or(0)))
        })
    });
    usage.cache_read = usage.cache_read.or_else(|| {
        count("/prompt_tokens_details/cached_tokens")
            .or_else(|| count("/num_cached_tokens"))
            .or_else(|| count("/prompt_cache_hit_tokens"))
    });
    usage.cache_write = usage
        .cache_write
        .or_else(|| count("/prompt_tokens_details/cache_write_tokens"));
    usage.reasoning = usage
        .reasoning
        .or_else(|| count("/completion_tokens_details/reasoning_tokens"));
    usage.provider.get_or_insert_with(|| raw.clone());
    usage
}
