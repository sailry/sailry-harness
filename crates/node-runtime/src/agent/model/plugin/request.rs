use super::super::types::{Content, Image, Request, Role};
use sailry_protocol::{
    Effort, Fault,
    plugin::model_adapter::{Capabilities, Dialect, wire},
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Context<'a> {
    pub endpoint: &'a str,
    pub session: &'a str,
    pub effort: Effort,
    pub features: BTreeMap<String, String>,
    pub capabilities: &'a Capabilities,
    pub dialect: Dialect,
}

pub(super) fn convert(source: &Request, context: Context<'_>) -> Result<wire::Request, Fault> {
    let Context {
        endpoint,
        session,
        effort,
        features,
        capabilities,
        dialect,
    } = context;
    if (!capabilities.reasoning && !matches!(effort, Effort::Default | Effort::Disabled))
        || (!capabilities.tools && !source.tools.is_empty())
    {
        return Err(unsupported());
    }
    if source.temperature.is_some_and(|value| !value.is_finite())
        || source
            .params
            .get("top_p")
            .and_then(Value::as_f64)
            .is_some_and(|value| !value.is_finite())
    {
        return Err(Fault::invalid("model request options are invalid"));
    }
    let mut names = BTreeSet::new();
    let tools = source
        .tools
        .iter()
        .map(|tool| {
            if dialect == Dialect::OpenAi && !names.insert(&tool.name) {
                return Err(Fault::invalid("model tool schema is invalid"));
            }
            Ok(wire::Tool {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            })
        })
        .collect::<Result<_, Fault>>()?;
    let mut output = Vec::new();
    let mut calls = BTreeMap::new();
    for message in &source.messages {
        if !message.metadata.agent_visible {
            continue;
        }
        let assistant = message.role == Role::Assistant;
        let mut parts = Vec::new();
        for block in &message.content {
            match block {
                Content::Thinking { .. } | Content::RedactedThinking { .. }
                    if !capabilities.reasoning =>
                {
                    return Err(unsupported());
                }
                Content::Image { .. } if !capabilities.vision => return Err(unsupported()),
                Content::Document { .. } if !capabilities.files => return Err(unsupported()),
                Content::ToolCall(_) | Content::ToolResult(_) if !capabilities.tools => {
                    return Err(unsupported());
                }
                _ => {}
            }
            match block {
                Content::Text { text } => parts.push(wire::Part::Text { text: text.clone() }),
                Content::Thinking { text, signature } => parts.push(wire::Part::Reasoning {
                    text: Some(text.clone()),
                    replay: signature.clone().filter(|signature| !signature.is_empty()),
                }),
                Content::RedactedThinking { data } => parts.push(wire::Part::Reasoning {
                    text: None,
                    replay: Some(data.clone()),
                }),
                Content::Image {
                    media_type, data, ..
                } => parts.push(wire::Part::Image {
                    media_type: media_type.clone(),
                    data: match data {
                        Image::Inline { data_base64 } => wire::Image::Inline {
                            data_base64: data_base64.clone(),
                        },
                        Image::Url { url } => wire::Image::Url { url: url.clone() },
                    },
                }),
                Content::Document {
                    media_type,
                    name,
                    data_base64,
                } => parts.push(wire::Part::File {
                    media_type: media_type.clone(),
                    name: name.clone(),
                    data_base64: data_base64.clone(),
                }),
                Content::ToolCall(call) if call.error.is_none() => {
                    calls.insert(call.id.clone(), call.name.clone());
                    parts.push(wire::Part::ToolCall {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                    });
                }
                Content::ToolResult(response) => {
                    flush(&mut output, &mut parts, assistant);
                    let (value, is_error) = match &response.result {
                        Ok(result) => (
                            serde_json::to_value(result).unwrap(),
                            result.is_error.unwrap_or(false),
                        ),
                        Err(error) => (serde_json::to_value(error).unwrap(), true),
                    };
                    output.push(wire::Message::Tool {
                        call_id: response.id.clone(),
                        name: calls
                            .get(&response.id)
                            .cloned()
                            .unwrap_or_else(|| response.name.clone()),
                        parts: vec![wire::Part::Json { value }],
                        is_error,
                    });
                }
                _ => {}
            }
        }
        flush(&mut output, &mut parts, assistant);
    }
    let mut params = source.params.clone();
    params.retain(|key, _| {
        !matches!(
            key.as_str(),
            "thinking_effort"
                | "disable_prompt_cache"
                | "cache_ttl"
                | "emit_clear_thinking"
                | "preserve_thinking_context"
                | "preserve_unsigned_thinking"
                | "thinking_budget"
                | "budget_tokens"
                | "enable_thinking"
                | "sailry_effort"
        )
    });
    Ok(wire::Request {
        version: "v1".into(),
        model: source.model.clone(),
        messages: output,
        system: (!source.system.is_empty()).then(|| source.system.clone()),
        tools,
        tool_choice: wire::Choice::Auto,
        reasoning: wire::Reasoning {
            effort,
            replay: capabilities.reasoning,
        },
        features,
        max_output_tokens: source.max_tokens,
        temperature: source.temperature,
        top_p: params.get("top_p").and_then(Value::as_f64),
        stop: params
            .get("stop")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default(),
        stream: true,
        session: session.into(),
        endpoint: endpoint.into(),
        options: params
            .into_iter()
            .filter(|(key, _)| !matches!(key.as_str(), "top_p" | "stop"))
            .collect(),
    })
}

fn unsupported() -> Fault {
    Fault::invalid("model adapter does not support the requested capability")
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
