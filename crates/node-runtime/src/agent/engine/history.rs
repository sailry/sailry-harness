//! Conversion at the GDK execution boundary. Durable history remains Sailry-owned.
mod media;
mod metadata;
#[cfg(test)]
mod tests;

use crate::agent::canonical::*;
use crate::store::{agent::blobs, database::storage_error};
use base64::Engine as _;
use goose_provider_types::conversation::message::{self as gdk, MessageContent};
use rmcp::model::{CallToolRequestParams, CallToolResult, ErrorData};
use sailry_protocol::{Fault, TurnId};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use turso::Connection;

pub(crate) fn provider_usage(
    source: &goose_provider_types::conversation::token_usage::ProviderUsage,
) -> Result<Usage, Fault> {
    let mut usage = metadata::usage(&gdk::MessageUsage::from_provider_usage(source, false));
    usage.reasoning = source
        .additional_data
        .as_ref()
        .and_then(|data| data.get("reasoning_tokens"))
        .and_then(Value::as_u64);
    Ok(usage)
}

pub(crate) async fn capture(
    db: &Connection,
    turn: TurnId,
    source: &gdk::Message,
    calls: &BTreeMap<String, String>,
) -> Result<Record, Fault> {
    let mut metadata = metadata::capture(&source.metadata, source.created);
    let mut message = Message {
        role: if source.role == rmcp::model::Role::User {
            Role::User
        } else {
            Role::Assistant
        },
        part_count: source.content.len(),
        usage: source.metadata.usage.as_deref().map(metadata::usage),
        ..Message::default()
    };
    for (index, content) in source.content.iter().enumerate() {
        let kind = match content {
            MessageContent::Text(text) => BlockKind::Text {
                text: text.text.clone(),
            },
            MessageContent::Image(image) => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&image.data)
                    .map_err(storage_error)?;
                let mut asset = blobs::put(db, &image.mime_type, &bytes).await?;
                asset.annotations = image
                    .annotations
                    .as_ref()
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(storage_error)?;
                BlockKind::Image { asset }
            }
            MessageContent::Document(document) => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&document.data)
                    .map_err(storage_error)?;
                let mut asset = blobs::put(db, &document.mime_type, &bytes).await?;
                asset.uri = document.name.clone();
                BlockKind::Document { asset }
            }
            MessageContent::ToolRequest(request) => {
                if request.id.is_empty() {
                    return Err(Fault::invalid("tool call identity is missing"));
                }
                let (name, arguments, error) = match &request.tool_call {
                    Ok(call) => (
                        call.name.to_string(),
                        Value::Object(call.arguments.clone().unwrap_or_default()),
                        None,
                    ),
                    Err(error) => {
                        let value = serde_json::to_value(error).map_err(storage_error)?;
                        (
                            String::new(),
                            Value::Null,
                            Some(serde_json::from_value(value).map_err(storage_error)?),
                        )
                    }
                };
                BlockKind::ToolRequest(ToolRequest {
                    id: request.id.clone(),
                    name,
                    arguments,
                    provider: request.metadata.clone(),
                    tool_meta: request.tool_meta.clone(),
                    error,
                })
            }
            MessageContent::ToolResponse(response) => {
                if response.id.is_empty() {
                    return Err(Fault::invalid("tool response identity is missing"));
                }
                let name = calls.get(&response.id).cloned().unwrap_or_default();
                let (value, format, status, meta) = match &response.tool_result {
                    Ok(result) => {
                        let mut result = result.clone();
                        if let Some(value) = result
                            .meta
                            .as_mut()
                            .and_then(|meta| meta.remove("sailry_context"))
                        {
                            metadata.context =
                                Some(serde_json::from_value(value).map_err(storage_error)?);
                        }
                        let status = if result.is_error == Some(true) {
                            ToolStatus::Error
                        } else {
                            ToolStatus::Success
                        };
                        if let Some(value) = &result.structured_content
                            && {
                                let plain = CallToolResult::structured(value.clone());
                                result.is_error == plain.is_error
                                    && result.content == plain.content
                                    && result.result_type == plain.result_type
                            }
                        {
                            (
                                value.clone(),
                                ToolFormat::Json,
                                status,
                                result.meta.map(|meta| meta.0),
                            )
                        } else {
                            (
                                serde_json::to_value(result).map_err(storage_error)?,
                                ToolFormat::Mcp,
                                status,
                                None,
                            )
                        }
                    }
                    Err(error) => (
                        serde_json::to_value(error).map_err(storage_error)?,
                        ToolFormat::McpError,
                        ToolStatus::Error,
                        None,
                    ),
                };
                let revision = blake3::hash(value.to_string().as_bytes())
                    .to_hex()
                    .to_string();
                let payload = blobs::capture(db, value).await?;
                let mut attachments = Vec::new();
                if format == ToolFormat::Mcp
                    && let Ok(result) = &response.tool_result
                {
                    for (index, content) in result.content.iter().enumerate() {
                        let rmcp::model::ContentBlock::Image(image) = content else {
                            continue;
                        };
                        if let Some(slot) = payload
                            .assets
                            .iter()
                            .find(|slot| slot.pointer == format!("/content/{index}/data"))
                        {
                            let mut asset = slot.asset.clone();
                            asset.annotations = image
                                .annotations
                                .as_ref()
                                .map(serde_json::to_value)
                                .transpose()
                                .map_err(storage_error)?;
                            attachments.push(Attachment::Inline { asset });
                        }
                    }
                }
                BlockKind::ToolResponse(ToolResponse {
                    id: response.id.clone(),
                    name,
                    status,
                    result: payload,
                    revision,
                    attachments,
                    provider: response.metadata.clone(),
                    format,
                    meta,
                })
            }
            MessageContent::Thinking(thinking) => BlockKind::Thinking {
                text: thinking.thinking.clone(),
                signature: Some(thinking.signature.clone()),
            },
            MessageContent::RedactedThinking(thinking) => BlockKind::RedactedThinking {
                data: vec![thinking.data.clone()],
                before: vec![source.content.get(index + 1).map(|_| index + 1)],
            },
            MessageContent::Error(error) => {
                message.error = Some(ResponseError {
                    code: serde_json::to_value(error.kind)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_owned)),
                    message: error.message.clone(),
                });
                message.finish = Some(Finish::Failed);
                continue;
            }
            // Approval/elicitation requests have their own typed Node notice owner.
            MessageContent::ToolConfirmationRequest(_) | MessageContent::ActionRequired(_) => {
                continue;
            }
            MessageContent::SystemNotification(notification) => {
                if notification.notification_type == gdk::SystemNotificationType::CreditsExhausted {
                    message.error = Some(ResponseError {
                        code: Some("credits_exhausted".into()),
                        message: notification.msg.clone(),
                    });
                    message.finish = Some(Finish::Failed);
                }
                continue;
            }
        };
        message.content.push(Block { index, kind });
    }
    let mut record = Record::new(turn, Kind::Message(message));
    record.id = source
        .id
        .clone()
        .ok_or_else(|| Fault::invalid("message identity is missing"))?;
    // Canonical receipt time keeps its precision; GDK's created field is seconds only.
    // GDK roles describe model input; these effects are generated by the agent.
    // Admitted human inputs retain their own canonical user provenance.
    record.provenance.author = "assistant".into();
    record.metadata = metadata;
    Ok(record)
}

pub(crate) async fn project(
    db: &Connection,
    record: &Record,
) -> Result<Option<gdk::Message>, Fault> {
    let message = match &record.kind {
        Kind::Message(message) => message,
        Kind::Compaction(compaction) => &compaction.summary,
        _ => return Ok(None),
    };
    let mut output = if message.role == Role::User {
        gdk::Message::user()
    } else {
        gdk::Message::assistant()
    };
    output.id = Some(record.id.clone());
    output.created = record
        .metadata
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.created)
        .unwrap_or_else(|| record.created.timestamp());
    output.metadata = metadata::project(record);
    let mut redacted = BTreeMap::<usize, Vec<String>>::new();
    for block in &message.content {
        if let BlockKind::RedactedThinking { data, before } = &block.kind {
            for (index, data) in data.iter().enumerate() {
                redacted
                    .entry(
                        before
                            .get(index)
                            .copied()
                            .flatten()
                            .unwrap_or(message.part_count),
                    )
                    .or_default()
                    .push(data.clone());
            }
        }
    }
    for index in 0..=message.part_count {
        for data in redacted.remove(&index).unwrap_or_default() {
            output.content.push(MessageContent::RedactedThinking(
                gdk::RedactedThinkingContentBlock { data },
            ));
        }
        let Some(block) = message.content.iter().find(|block| block.index == index) else {
            continue;
        };
        let content = match &block.kind {
            BlockKind::Text { text } => {
                if record
                    .metadata
                    .input
                    .as_ref()
                    .and_then(|input| input.capability_part)
                    == Some(block.index)
                {
                    continue;
                }
                MessageContent::text(text)
            }
            BlockKind::Image { asset } => {
                let bytes = blobs::get(db, &asset.digest).await?;
                let mut image = rmcp::model::ImageContent::new(
                    base64::engine::general_purpose::STANDARD.encode(bytes),
                    asset.mime.clone(),
                );
                image.annotations = asset
                    .annotations
                    .as_ref()
                    .map(|value| serde_json::from_value(value.clone()))
                    .transpose()
                    .map_err(storage_error)?;
                MessageContent::Image(image)
            }
            BlockKind::Document { asset } => MessageContent::Document(gdk::DocumentContent {
                data: base64::engine::general_purpose::STANDARD
                    .encode(blobs::get(db, &asset.digest).await?),
                mime_type: asset.mime.clone(),
                name: asset.uri.clone(),
            }),
            // SessionLoader resolves Node attachments outside the storage actor.
            BlockKind::File { file } => MessageContent::Document(gdk::DocumentContent {
                data: file.uri.clone(),
                mime_type: file.mime.clone().unwrap_or_default(),
                name: None,
            }),
            BlockKind::Thinking { text, signature } => {
                MessageContent::Thinking(gdk::ThinkingContentBlock {
                    thinking: text.clone(),
                    signature: signature.clone().unwrap_or_default(),
                })
            }
            BlockKind::ToolRequest(request) => {
                let id = &request.id;
                let tool_call = match &request.error {
                    Some(error) => Err(serde_json::from_value::<ErrorData>(
                        serde_json::to_value(error).map_err(storage_error)?,
                    )
                    .map_err(storage_error)?),
                    None => Ok(
                        CallToolRequestParams::new(request.name.clone()).with_arguments(
                            request.arguments.as_object().cloned().unwrap_or_default(),
                        ),
                    ),
                };
                MessageContent::ToolRequest(gdk::ToolRequest {
                    id: id.clone(),
                    tool_call,
                    metadata: request.provider.clone(),
                    tool_meta: request.tool_meta.clone(),
                })
            }
            BlockKind::ToolResponse(response) => {
                let id = &response.id;
                let value = blobs::hydrate(db, &response.result).await?;
                let tool_result = match response.format {
                    ToolFormat::McpError => {
                        Err(serde_json::from_value(value).map_err(storage_error)?)
                    }
                    ToolFormat::Mcp => Ok(serde_json::from_value(value).map_err(storage_error)?),
                    ToolFormat::Json => {
                        let mut result = media::project(db, value, &response.attachments).await?;
                        result.meta = response.meta.clone().map(rmcp::model::MetaObject);
                        if response.status != ToolStatus::Success {
                            result.is_error = Some(true);
                        }
                        Ok(result)
                    }
                };
                MessageContent::ToolResponse(gdk::ToolResponse {
                    id: id.clone(),
                    tool_result,
                    metadata: response.provider.clone(),
                })
            }
            BlockKind::RedactedThinking { .. } => continue,
        };
        output.content.push(content);
    }
    if let Some(error) = &message.error {
        output
            .content
            .push(MessageContent::Error(gdk::ErrorContent {
                kind: error
                    .code
                    .as_ref()
                    .and_then(|code| serde_json::from_value(json!(code)).ok())
                    .unwrap_or(gdk::MessageErrorKind::Other),
                message: error.message.clone(),
            }));
    }
    if output.content.is_empty() && !output.is_user_visible() && !output.is_agent_visible() {
        return Ok(None);
    }
    Ok(Some(output))
}
