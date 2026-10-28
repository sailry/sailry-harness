//! A captured package receives neutral messages from the GDK provider boundary.
use goose_provider_types::{
    conversation::message::{Message, MessageContentBlock},
    model::ModelConfig,
};
use sailry_protocol::{
    Effort, Fault,
    plugin::model_adapter::{Capabilities, Dialect, wire},
};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn convert(
    config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[rmcp::model::Tool],
    endpoint: &str,
    session: &str,
    effort: Effort,
    features: BTreeMap<String, String>,
    capabilities: &Capabilities,
    dialect: Dialect,
) -> Result<wire::Request, Fault> {
    let schemas: Vec<Value> = match dialect {
        Dialect::OpenAi => goose_provider_types::formats::openai::format_tools(tools)
            .map_err(|_| Fault::invalid("model tool schema is invalid"))?
            .into_iter()
            .filter_map(|tool| tool.get("function").cloned())
            .collect(),
        Dialect::Gemini => goose_provider_types::formats::google::format_tools(tools),
        Dialect::Generic => Vec::new(),
    };
    let tools = tools
        .iter()
        .enumerate()
        .map(|(index, tool)| {
            let schema = schemas.get(index);
            wire::Tool {
                name: tool.name.to_string(),
                description: tool.description.as_ref().map(ToString::to_string),
                parameters: schema
                    .and_then(|schema| {
                        schema
                            .get("parameters")
                            .or_else(|| schema.get("parametersJsonSchema"))
                    })
                    .cloned()
                    .unwrap_or_else(|| serde_json::to_value(&tool.input_schema).unwrap()),
            }
        })
        .collect();
    let mut output = Vec::new();
    for message in messages {
        let assistant = message.role == rmcp::model::Role::Assistant;
        let mut parts = Vec::new();
        for block in &message.content {
            match block {
                MessageContentBlock::Text(text) => parts.push(wire::Part::Text {
                    text: text.text.clone(),
                }),
                MessageContentBlock::Thinking(thinking) => parts.push(wire::Part::Reasoning {
                    text: Some(thinking.thinking.clone()),
                    replay: (!thinking.signature.is_empty()).then(|| thinking.signature.clone()),
                }),
                MessageContentBlock::RedactedThinking(thinking) => {
                    parts.push(wire::Part::Reasoning {
                        text: None,
                        replay: Some(thinking.data.clone()),
                    })
                }
                MessageContentBlock::Image(image) => parts.push(wire::Part::Image {
                    media_type: image.mime_type.clone(),
                    data: if image.data.starts_with("https://") || image.data.starts_with("http://")
                    {
                        wire::Image::Url {
                            url: image.data.clone(),
                        }
                    } else {
                        wire::Image::Inline {
                            data_base64: image.data.clone(),
                        }
                    },
                }),
                MessageContentBlock::Document(document) => parts.push(wire::Part::File {
                    media_type: document.mime_type.clone(),
                    name: document.name.clone(),
                    data_base64: document.data.clone(),
                }),
                MessageContentBlock::ToolRequest(request) => {
                    if let Ok(call) = &request.tool_call {
                        parts.push(wire::Part::ToolCall {
                            id: request.id.clone(),
                            name: call.name.to_string(),
                            arguments: serde_json::to_value(&call.arguments).unwrap_or(Value::Null),
                        });
                    }
                }
                MessageContentBlock::ToolResponse(response) => {
                    flush(&mut output, &mut parts, assistant);
                    let (value, is_error) = match &response.tool_result {
                        Ok(result) => (
                            serde_json::to_value(result).unwrap(),
                            result.is_error.unwrap_or(false),
                        ),
                        Err(error) => (serde_json::to_value(error).unwrap(), true),
                    };
                    let name = messages
                        .iter()
                        .flat_map(|message| &message.content)
                        .find_map(|block| {
                            let MessageContentBlock::ToolRequest(request) = block else {
                                return None;
                            };
                            (request.id == response.id)
                                .then(|| {
                                    request
                                        .tool_call
                                        .as_ref()
                                        .ok()
                                        .map(|call| call.name.to_string())
                                })
                                .flatten()
                        })
                        .unwrap_or_default();
                    output.push(wire::Message::Tool {
                        call_id: response.id.clone(),
                        name,
                        parts: vec![wire::Part::Json { value }],
                        is_error,
                    });
                }
                _ => {}
            }
        }
        flush(&mut output, &mut parts, assistant);
    }
    let params = config.request_params.clone().unwrap_or_default();
    Ok(wire::Request {
        version: "v1".into(),
        model: config.model_name.clone(),
        messages: output,
        system: (!system.is_empty()).then(|| system.into()),
        tools,
        // The v1 package input retains this field; Sailry never selects a tool.
        tool_choice: wire::Choice::Auto,
        reasoning: wire::Reasoning {
            effort,
            replay: capabilities.reasoning,
        },
        features,
        max_output_tokens: config
            .max_tokens
            .and_then(|value| u64::try_from(value).ok()),
        temperature: config.temperature.map(f64::from),
        top_p: params.get("top_p").and_then(Value::as_f64),
        stop: params
            .get("stop")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default(),
        stream: true,
        session: session.into(),
        endpoint: endpoint.into(),
        options: params.into_iter().collect(),
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
