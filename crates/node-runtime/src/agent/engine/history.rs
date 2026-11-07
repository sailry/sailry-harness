//! Conversion between canonical history and the Sailry model boundary.
mod cache;
mod media;
mod metadata;
#[cfg(test)]
mod tests;

use crate::agent::canonical::*;
use crate::agent::model::types::{self as model, Content};
use crate::store::{agent::blobs, database::storage_error};
use base64::Engine as _;
use rmcp::model::CallToolResult;
use sailry_protocol::{Fault, TurnId};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use turso::Connection;

pub(crate) use cache::Cache;

pub(crate) fn provider_usage(source: &model::Usage) -> Result<Usage, Fault> {
    Ok(metadata::usage(source))
}

pub(crate) async fn capture(
    db: &Connection,
    turn: TurnId,
    source: &model::Message,
    calls: &BTreeMap<String, String>,
) -> Result<Record, Fault> {
    let mut metadata = metadata::capture(&source.metadata, source.created);
    let mut message = Message {
        role: if source.role == model::Role::User {
            Role::User
        } else {
            Role::Assistant
        },
        part_count: source.content.len(),
        model: source
            .metadata
            .usage
            .as_deref()
            .and_then(|usage| usage.model.clone()),
        usage: source.metadata.usage.as_deref().map(metadata::usage),
        ..Message::default()
    };
    for (index, content) in source.content.iter().enumerate() {
        let kind = match content {
            Content::Text { text } => BlockKind::Text { text: text.clone() },
            Content::Image {
                media_type,
                data,
                annotations,
            } => {
                let model::Image::Inline { data_base64 } = data else {
                    return Err(Fault::invalid("history image requires inline data"));
                };
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data_base64)
                    .map_err(storage_error)?;
                let mut asset = blobs::put(db, media_type, &bytes).await?;
                asset.annotations = annotations.clone();
                BlockKind::Image { asset }
            }
            Content::Document {
                media_type,
                name,
                data_base64,
            } => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data_base64)
                    .map_err(storage_error)?;
                let mut asset = blobs::put(db, media_type, &bytes).await?;
                asset.uri = name.clone();
                BlockKind::Document { asset }
            }
            Content::ToolCall(request) => {
                if request.id.is_empty() {
                    return Err(Fault::invalid("tool call identity is missing"));
                }
                BlockKind::ToolRequest(ToolRequest {
                    id: request.id.clone(),
                    name: request.name.clone(),
                    arguments: request.arguments.clone(),
                    provider: request.metadata.clone(),
                    tool_meta: request.tool_meta.clone(),
                    error: request.error.as_ref().map(|error| ToolError {
                        code: i64::from(error.code),
                        message: error.message.clone(),
                        data: error.data.clone(),
                    }),
                })
            }
            Content::ToolResult(response) => {
                if response.id.is_empty() {
                    return Err(Fault::invalid("tool response identity is missing"));
                }
                let name = calls
                    .get(&response.id)
                    .cloned()
                    .unwrap_or_else(|| response.name.clone());
                let (value, format, status, meta) = match &response.result {
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
                    && let Ok(result) = &response.result
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
            Content::Thinking { text, signature } => BlockKind::Thinking {
                text: text.clone(),
                signature: signature.clone(),
            },
            Content::RedactedThinking { data } => BlockKind::RedactedThinking {
                data: vec![data.clone()],
                before: vec![source.content.get(index + 1).map(|_| index + 1)],
            },
            Content::Error {
                code,
                message: text,
            } => {
                message.error = Some(ResponseError {
                    code: code.clone(),
                    message: text.clone(),
                });
                message.finish = Some(Finish::Failed);
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
    // Canonical receipt time keeps its precision; model timestamps are seconds only.
    // Model roles describe input; these effects are generated by the agent.
    // Admitted human inputs retain their own canonical user provenance.
    record.provenance.author = "assistant".into();
    record.metadata = metadata;
    Ok(record)
}

pub(crate) async fn project(
    db: &Connection,
    record: &Record,
) -> Result<Option<model::Message>, Fault> {
    let message = match &record.kind {
        Kind::Message(message) => message,
        Kind::Compaction(compaction) => &compaction.summary,
        _ => return Ok(None),
    };
    let mut output = model::Message {
        role: if message.role == Role::User {
            model::Role::User
        } else {
            model::Role::Assistant
        },
        ..model::Message::default()
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
            output.content.push(Content::RedactedThinking { data });
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
                Content::Text { text: text.clone() }
            }
            BlockKind::Image { asset } => {
                let bytes = blobs::get(db, &asset.digest).await?;
                Content::Image {
                    media_type: asset.mime.clone(),
                    data: model::Image::Inline {
                        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                    },
                    annotations: asset.annotations.clone(),
                }
            }
            BlockKind::Document { asset } => Content::Document {
                data_base64: base64::engine::general_purpose::STANDARD
                    .encode(blobs::get(db, &asset.digest).await?),
                media_type: asset.mime.clone(),
                name: asset.uri.clone(),
            },
            // SessionLoader resolves Node attachments outside the storage actor.
            BlockKind::File { file } => Content::Document {
                data_base64: file.uri.clone(),
                media_type: file.mime.clone().unwrap_or_default(),
                name: None,
            },
            BlockKind::Thinking { text, signature } => Content::Thinking {
                text: text.clone(),
                signature: signature.clone(),
            },
            BlockKind::ToolRequest(request) => Content::ToolCall(model::ToolCall {
                id: request.id.clone(),
                name: request.name.clone(),
                arguments: request.arguments.clone(),
                metadata: request.provider.clone(),
                tool_meta: request.tool_meta.clone(),
                error: request
                    .error
                    .as_ref()
                    .map(|error| {
                        Ok::<_, Fault>(model::ToolError {
                            code: i32::try_from(error.code).map_err(storage_error)?,
                            message: error.message.clone(),
                            data: error.data.clone(),
                        })
                    })
                    .transpose()?,
            }),
            BlockKind::ToolResponse(response) => {
                // A derived reference survives media hydration without hashing a
                // modified payload. The canonical writer ignores this note.
                output.metadata.set_operation_note(
                    "tool_output",
                    &response.id,
                    json!(response.revision),
                );
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
                Content::ToolResult(model::ToolResult {
                    id: id.clone(),
                    name: response.name.clone(),
                    result: tool_result,
                    metadata: response.provider.clone(),
                })
            }
            BlockKind::RedactedThinking { .. } => continue,
        };
        output.content.push(content);
    }
    if let Some(error) = &message.error {
        output.content.push(Content::Error {
            code: error.code.clone(),
            message: error.message.clone(),
        });
    }
    if output.content.is_empty() && !output.metadata.user_visible && !output.metadata.agent_visible
    {
        return Ok(None);
    }
    Ok(Some(output))
}
