use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use goose_provider_types::{model::ModelConfig, thinking::GEMINI_THOUGHT_SIGNATURE_KEY};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock as ToolContent, ErrorData, Tool,
};
use serde_json::Value;

pub(super) fn convert(
    request: LlmRequest,
    frozen: &ModelConfig,
) -> adk_core::Result<(ModelConfig, String, Vec<Message>, Vec<Tool>)> {
    let mut config = frozen.clone();
    if let Some(options) = request.config {
        if let Some(max) = options.max_output_tokens {
            config.max_tokens = Some(max);
        }
        if let Some(temperature) = options.temperature {
            config.temperature = Some(temperature);
        }
        let mut params = std::collections::HashMap::new();
        if let Some(top_p) = options.top_p {
            params.insert("top_p".into(), json!(top_p));
        }
        if !options.stop_sequences.is_empty() {
            params.insert("stop".into(), json!(options.stop_sequences));
        }
        if let Some(schema) = options.response_schema {
            params.insert(
                "response_format".into(),
                json!({"type":"json_schema","json_schema":{"name":"response","schema":schema}}),
            );
        }
        for namespace in ["openai", "anthropic", "google", "gemini", "deepseek"] {
            if let Some(fields) = options.extensions.get(namespace).and_then(Value::as_object) {
                for (key, value) in fields {
                    if !matches!(
                        key.as_str(),
                        "include" | "previous_response_id" | "tool_choice"
                    ) {
                        params.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        config = config.with_merged_request_params(params);
    }
    let mut system = Vec::new();
    let mut messages = Vec::new();
    for content in request.contents {
        if matches!(content.role.as_str(), "system" | "developer") {
            system.extend(content.parts.into_iter().filter_map(|part| match part {
                Part::Text { text } => Some(text),
                _ => None,
            }));
            continue;
        }
        let mut message = if matches!(content.role.as_str(), "model" | "assistant") {
            Message::assistant()
        } else {
            Message::user()
        };
        let native = content.parts.iter().find_map(|part| match part {
            Part::ServerToolResponse {
                server_tool_response,
            } if server_tool_response["type"] == "anthropic_message" => {
                server_tool_response["content"].as_array()
            }
            _ => None,
        });
        if let Some(native) = native {
            // The Stage 1 bridge reconstructs this transient native message in order;
            // consume it once rather than duplicating its ordinary ADK parts.
            for block in native {
                match block["type"].as_str() {
                    Some("text") => message.content.push(MessageContentBlock::text(
                        block["text"].as_str().unwrap_or_default(),
                    )),
                    Some("thinking") => message.content.push(MessageContentBlock::thinking(
                        block["thinking"].as_str().unwrap_or_default(),
                        block["signature"].as_str().unwrap_or_default(),
                    )),
                    Some("redacted_thinking") => {
                        message.content.push(MessageContentBlock::redacted_thinking(
                            block["data"].as_str().unwrap_or_default(),
                        ))
                    }
                    Some("tool_use") => message.content.push(call(
                        block["id"].as_str().map(str::to_owned),
                        block["name"].as_str().unwrap_or_default().into(),
                        block["input"].clone(),
                        None,
                    )),
                    _ => {}
                }
            }
        } else {
            for part in content.parts {
                append(&mut message.content, part)?;
            }
        }
        if !message.content.is_empty() {
            messages.push(message);
        }
    }
    let mut declarations: Vec<_> = request
        .tools
        .into_iter()
        .filter_map(|(name, declaration)| {
            // Hosted ADK-native declarations have no GDK function equivalent.
            if declaration.get("x-adk-openai-tool").is_some()
                || declaration.get("x-adk-anthropic-tool").is_some()
                || declaration.get("x-adk-gemini-tool").is_some()
            {
                return None;
            }
            let schema = declaration
                .get("parameters")
                .unwrap_or(&declaration)
                .as_object()
                .cloned()
                .unwrap_or_default();
            Some(Tool::new(
                name,
                declaration
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                schema,
            ))
        })
        .collect();
    declarations.sort_by(|left, right| left.name.cmp(&right.name));
    Ok((config, system.join("\n\n"), messages, declarations))
}

fn append(blocks: &mut Vec<MessageContentBlock>, part: Part) -> adk_core::Result<()> {
    match part {
        Part::ConfigurationUpdate { .. } => {}
        Part::ProviderData { provider_data } => {
            let [part] = *provider_data.parts;
            append(blocks, part)?;
        }
        Part::Text { text } => blocks.push(MessageContentBlock::text(text)),
        Part::Thinking {
            thinking,
            signature,
        } => blocks.push(MessageContentBlock::thinking(
            thinking,
            signature.unwrap_or_default(),
        )),
        Part::FunctionCall {
            name,
            args,
            id,
            thought_signature,
        } => blocks.push(call(id, name, args, thought_signature)),
        Part::FunctionResponse {
            function_response,
            id,
            ..
        } => {
            let value = function_response.response;
            let result: CallToolResult =
                serde_json::from_value(value.clone()).unwrap_or_else(|_| {
                    CallToolResult::success(vec![ToolContent::text(
                        serde_json::to_string(&value).unwrap_or_default(),
                    )])
                });
            blocks.push(MessageContentBlock::tool_response(
                id.unwrap_or_default(),
                Ok(result),
            ));
        }
        Part::InlineData {
            mime_type, data, ..
        } => {
            let data = STANDARD.encode(data);
            if mime_type.starts_with("image/") {
                blocks.push(MessageContentBlock::image(data, mime_type));
            } else {
                blocks.push(MessageContentBlock::document(data, mime_type, None));
            }
        }
        Part::FileData {
            mime_type,
            file_uri,
            ..
        } => {
            // GDK ImageContent contains base64 bytes, not a remote URI. Existing
            // attachment projection supplies inline data for model-visible media.
            let _ = mime_type;
            blocks.push(MessageContentBlock::text(file_uri));
        }
        Part::EmbeddedResource {
            resource: adk_core::EmbeddedResource::Text(resource),
        } => blocks.push(MessageContentBlock::text(resource.text)),
        Part::EmbeddedResource {
            resource: adk_core::EmbeddedResource::Blob(resource),
        } => blocks.push(MessageContentBlock::document(
            STANDARD.encode(resource.data),
            resource
                .mime_type
                .unwrap_or_else(|| "application/octet-stream".into()),
            None,
        )),
        Part::ServerToolCall { .. } | Part::ServerToolResponse { .. } => {}
        _ => {}
    }
    Ok(())
}

fn call(
    id: Option<String>,
    name: String,
    args: Value,
    signature: Option<String>,
) -> MessageContentBlock {
    let tool_call = args
        .as_object()
        .cloned()
        .map(|arguments| CallToolRequestParams::new(name).with_arguments(arguments))
        .ok_or_else(|| ErrorData::invalid_params("tool arguments must be an object", Some(args)));
    let metadata = signature.map(|signature| {
        [(GEMINI_THOUGHT_SIGNATURE_KEY.into(), json!(signature))]
            .into_iter()
            .collect()
    });
    MessageContentBlock::tool_request_with_metadata(
        id.unwrap_or_default(),
        tool_call,
        metadata.as_ref(),
    )
}
