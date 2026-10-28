use super::*;
use adk_core::{FinishReason, UsageMetadata};
use base64::{Engine, engine::general_purpose::STANDARD};
use goose_provider_types::thinking::GEMINI_THOUGHT_SIGNATURE_KEY;

pub(super) fn convert(
    message: Message,
    usage: ProviderUsage,
    model: &str,
    provider: &str,
) -> adk_core::Result<LlmResponse> {
    let native = message.content.iter().any(|block| matches!(block, MessageContentBlock::RedactedThinking(_)))
        .then(|| message.content.iter().filter_map(|block| match block {
            MessageContentBlock::Text(text) => Some(json!({"type":"text","text":text.text})),
            MessageContentBlock::Thinking(thinking) => Some(json!({"type":"thinking","thinking":thinking.thinking,"signature":thinking.signature})),
            MessageContentBlock::RedactedThinking(thinking) => Some(json!({"type":"redacted_thinking","data":thinking.data})),
            MessageContentBlock::ToolRequest(request) => request.tool_call.as_ref().ok().map(|call|
                json!({"type":"tool_use","id":request.id,"name":call.name,"input":call.arguments})),
            _ => None,
        }).collect::<Vec<_>>());
    let mut parts = Vec::new();
    let mut tool_metadata = serde_json::Map::new();
    for block in message.content {
        match block {
            MessageContentBlock::Text(text) => parts.push(Part::Text { text: text.text }),
            MessageContentBlock::Thinking(thinking) => parts.push(Part::Thinking {
                thinking: thinking.thinking,
                signature: (!thinking.signature.is_empty()).then_some(thinking.signature),
            }),
            MessageContentBlock::ToolRequest(request) => {
                let call = request
                    .tool_call
                    .map_err(|error| AdkError::model(error.message.to_string()))?;
                let signature = request
                    .metadata
                    .as_ref()
                    .and_then(|fields| fields.get(GEMINI_THOUGHT_SIGNATURE_KEY))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                tool_metadata.insert(
                    request.id.clone(),
                    json!({"provider":request.metadata,"tool_meta":request.tool_meta}),
                );
                parts.push(Part::FunctionCall {
                    id: Some(request.id),
                    name: call.name.to_string(),
                    args: json!(call.arguments.unwrap_or_default()),
                    thought_signature: signature,
                });
            }
            MessageContentBlock::Image(image) => parts.push(Part::InlineData {
                mime_type: image.mime_type,
                data: STANDARD
                    .decode(image.data)
                    .map_err(|_| AdkError::model("model image is not base64"))?,
                uri: None,
                annotations: None,
            }),
            MessageContentBlock::Document(document) => parts.push(Part::InlineData {
                mime_type: document.mime_type,
                data: STANDARD
                    .decode(document.data)
                    .map_err(|_| AdkError::model("model document is not base64"))?,
                uri: None,
                annotations: None,
            }),
            MessageContentBlock::RedactedThinking(_) => {}
            MessageContentBlock::Error(error) => return Err(AdkError::model(error.message)),
            _ => {}
        }
    }
    if let Some(content) = native {
        // The temporary bridge extracts redacted blocks and their ordinary-part
        // anchors, so no duplicate native message remains in canonical storage.
        parts.push(Part::ServerToolResponse {
            server_tool_response: json!({"type":"anthropic_message","content":content}),
        });
    }
    let tools = parts
        .iter()
        .any(|part| matches!(part, Part::FunctionCall { .. }));
    let finish = if usage.finish_reasons.as_ref().is_some_and(|reasons| {
        reasons.iter().any(|reason| {
            matches!(
                reason.as_str(),
                "length" | "max_tokens" | "max_output_tokens"
            )
        })
    }) {
        FinishReason::MaxTokens
    } else if tools {
        FinishReason::ToolCalls
    } else {
        FinishReason::Stop
    };
    let cost = usage.cost;
    let raw = serde_json::to_value(&usage).ok();
    Ok(LlmResponse {
        content: Some(Content {
            role: "model".into(),
            parts,
        }),
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: usage.usage.input_tokens.unwrap_or(-1),
            candidates_token_count: usage.usage.output_tokens.unwrap_or(-1),
            total_token_count: usage.usage.total_tokens.unwrap_or(-1),
            cache_read_input_token_count: usage.usage.cache_read_input_tokens,
            cache_creation_input_token_count: usage.usage.cache_write_input_tokens,
            cost,
            thinking_token_count: usage
                .additional_data
                .as_ref()
                .and_then(|fields| fields.get("reasoning_tokens"))
                .and_then(serde_json::Value::as_i64)
                .and_then(|value| i32::try_from(value).ok()),
            provider_usage: raw,
            ..Default::default()
        }),
        finish_reason: Some(finish),
        turn_complete: true,
        provider_metadata: Some(json!({"content_complete":true,"gdk_tool_metadata":tool_metadata})),
        model: Some(if usage.model == "unknown" {
            model.into()
        } else {
            usage.model
        }),
        provider: Some(provider.into()),
        ..Default::default()
    })
}
