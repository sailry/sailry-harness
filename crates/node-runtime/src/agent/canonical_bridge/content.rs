use super::*;
use adk_core::{FileDataPart, FunctionResponseData, InlineDataPart, Part};

pub(super) async fn capture(db: &Connection, content: &Content) -> Result<Vec<Block>, Fault> {
    let mut blocks = Vec::new();
    for (index, original) in content.parts.iter().enumerate() {
        let part = super::native::ordinary(original);
        let kind = match part {
            Part::ConfigurationUpdate { .. } | Part::ProviderData { .. } => continue,
            Part::Text { text } => BlockKind::Text { text: text.clone() },
            Part::Thinking {
                thinking,
                signature,
            } => BlockKind::Thinking {
                text: thinking.clone(),
                signature: signature.clone(),
            },
            Part::InlineData {
                mime_type,
                data,
                uri,
                annotations,
            } => {
                let mut asset = blobs::put(db, mime_type, data).await?;
                asset.uri = uri.clone();
                asset.annotations = annotations.clone();
                if mime_type.starts_with("image/") {
                    BlockKind::Image { asset }
                } else {
                    BlockKind::Document { asset }
                }
            }
            Part::FileData {
                mime_type,
                file_uri,
                annotations,
            } => BlockKind::File {
                file: FileRef {
                    uri: file_uri.clone(),
                    mime: Some(mime_type.clone()),
                    annotations: annotations.clone(),
                },
            },
            Part::FunctionCall {
                name,
                args,
                id,
                thought_signature,
            } => BlockKind::ToolRequest(ToolRequest {
                id: id.clone(),
                name: name.clone(),
                arguments: args.clone(),
                signature: thought_signature.clone(),
                provider: thought_signature.as_ref().map(|signature| {
                    serde_json::Map::from_iter([("thoughtSignature".into(), json!(signature))])
                }),
                tool_meta: None,
                error: None,
            }),
            Part::FunctionResponse {
                function_response,
                id,
                annotations,
            } => {
                let raw = &function_response.response;
                let revision = blake3::hash(&serde_json::to_vec(raw).map_err(storage_error)?)
                    .to_hex()
                    .to_string();
                let mut attachments = Vec::new();
                for data in &function_response.inline_data {
                    let mut asset = blobs::put(db, &data.mime_type, &data.data).await?;
                    asset.uri = data.uri.clone();
                    asset.annotations = data.annotations.clone();
                    attachments.push(Attachment::Inline { asset });
                }
                attachments.extend(function_response.file_data.iter().map(|data| {
                    Attachment::File {
                        file: FileRef {
                            uri: data.file_uri.clone(),
                            mime: Some(data.mime_type.clone()),
                            annotations: data.annotations.clone(),
                        },
                    }
                }));
                let status = match part.tool_result_status() {
                    Some(adk_core::ToolResultStatus::Failed) => ToolStatus::Error,
                    Some(adk_core::ToolResultStatus::Pending) => ToolStatus::Pending,
                    Some(adk_core::ToolResultStatus::Cancelled) => ToolStatus::Cancelled,
                    Some(adk_core::ToolResultStatus::OutcomeUnknown) => ToolStatus::Unknown,
                    None => ToolStatus::Success,
                };
                BlockKind::ToolResponse(ToolResponse {
                    id: id.clone(),
                    name: function_response.name.clone(),
                    status,
                    result: blobs::capture(db, raw.clone()).await?,
                    revision,
                    attachments,
                    annotations: annotations.clone(),
                    provider: None,
                    format: ToolFormat::Json,
                    meta: None,
                })
            }
            Part::ServerToolCall { server_tool_call }
                if super::native::private(server_tool_call) =>
            {
                continue;
            }
            Part::ServerToolResponse {
                server_tool_response,
            } if server_tool_response["type"] == "anthropic_message" => {
                let Some(redacted) = super::native::capture(content, server_tool_response)? else {
                    continue;
                };
                redacted
            }
            Part::ServerToolResponse {
                server_tool_response,
            } if super::native::private(server_tool_response) => continue,
            Part::ServerToolCall { server_tool_call } => BlockKind::ServerCall {
                value: blobs::capture(db, server_tool_call.clone()).await?,
            },
            Part::ServerToolResponse {
                server_tool_response,
            } => BlockKind::ServerResponse {
                value: blobs::capture(db, server_tool_response.clone()).await?,
            },
            Part::EmbeddedResource { resource } => BlockKind::Resource {
                value: blobs::capture(db, serde_json::to_value(resource).map_err(storage_error)?)
                    .await?,
            },
        };
        blocks.push(Block { index, kind });
    }
    Ok(blocks)
}

pub(super) async fn restore(
    db: &Connection,
    message: &Message,
    provenance: &Provenance,
) -> Result<Content, Fault> {
    let length = message.part_count.max(
        message
            .content
            .iter()
            .map(|block| block.index + 1)
            .max()
            .unwrap_or(0),
    );
    // Empty slots exist only at the temporary ADK boundary. Canonical blocks keep
    // their source indices without storing discarded engine replay envelopes.
    let mut parts = vec![
        Part::Text {
            text: String::new()
        };
        length
    ];
    for block in &message.content {
        let part = match &block.kind {
            BlockKind::Text { text } => Part::Text { text: text.clone() },
            BlockKind::Thinking { text, signature } => Part::Thinking {
                thinking: text.clone(),
                signature: signature.clone(),
            },
            BlockKind::RedactedThinking { data, before } => Part::ServerToolResponse {
                server_tool_response: super::native::restore(db, message, data, before).await?,
            },
            BlockKind::Image { asset } | BlockKind::Document { asset } => Part::InlineData {
                mime_type: asset.mime.clone(),
                data: blobs::get(db, &asset.digest).await?,
                uri: asset.uri.clone(),
                annotations: asset.annotations.clone(),
            },
            BlockKind::File { file } => Part::FileData {
                mime_type: file.mime.clone().unwrap_or_default(),
                file_uri: file.uri.clone(),
                annotations: file.annotations.clone(),
            },
            BlockKind::ToolRequest(call) => Part::FunctionCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.arguments.clone(),
                thought_signature: call.signature.clone().or_else(|| {
                    call.provider
                        .as_ref()?
                        .get("thoughtSignature")?
                        .as_str()
                        .map(str::to_owned)
                }),
            },
            BlockKind::ToolResponse(result) => {
                let mut inline_data = Vec::new();
                let mut file_data = Vec::new();
                for attachment in &result.attachments {
                    match attachment {
                        Attachment::Inline { asset } => inline_data.push(InlineDataPart {
                            mime_type: asset.mime.clone(),
                            data: blobs::get(db, &asset.digest).await?,
                            uri: asset.uri.clone(),
                            annotations: asset.annotations.clone(),
                        }),
                        Attachment::File { file } => file_data.push(FileDataPart {
                            mime_type: file.mime.clone().unwrap_or_default(),
                            file_uri: file.uri.clone(),
                            annotations: file.annotations.clone(),
                        }),
                    }
                }
                Part::FunctionResponse {
                    id: result.id.clone(),
                    annotations: result.annotations.clone(),
                    function_response: FunctionResponseData {
                        name: result.name.clone(),
                        response: blobs::hydrate(db, &result.result).await?,
                        inline_data,
                        file_data,
                    },
                }
            }
            BlockKind::ServerCall { value } => Part::ServerToolCall {
                server_tool_call: blobs::hydrate(db, value).await?,
            },
            BlockKind::ServerResponse { value } => Part::ServerToolResponse {
                server_tool_response: blobs::hydrate(db, value).await?,
            },
            BlockKind::Resource { value } => Part::EmbeddedResource {
                resource: serde_json::from_value(blobs::hydrate(db, value).await?)
                    .map_err(storage_error)?,
            },
        };
        parts[block.index] = part;
    }
    let role = if message
        .content
        .iter()
        .any(|block| matches!(&block.kind, BlockKind::ToolResponse(_)))
        && provenance.author != "user"
    {
        "function"
    } else if message.role == Role::User {
        "user"
    } else {
        "model"
    };
    Ok(Content {
        role: role.into(),
        parts,
    })
}

// The temporary Llm adapter can receive metadata that ADK FunctionCall cannot
// express. Capture it immediately in the existing canonical tool fields.
pub(super) fn tool_metadata(blocks: &mut [Block], event: &Event) -> Result<(), Fault> {
    let Some(fields) = event
        .llm_response
        .provider_metadata
        .as_ref()
        .and_then(|metadata| metadata.get("gdk_tool_metadata"))
        .and_then(Value::as_object)
    else {
        return Ok(());
    };
    for block in blocks {
        let BlockKind::ToolRequest(call) = &mut block.kind else {
            continue;
        };
        let Some(value) = call.id.as_ref().and_then(|id| fields.get(id)) else {
            continue;
        };
        if let Some(provider) = value.get("provider").filter(|value| !value.is_null()) {
            call.provider = Some(serde_json::from_value(provider.clone()).map_err(storage_error)?);
        }
        call.tool_meta = value
            .get("tool_meta")
            .filter(|value| !value.is_null())
            .cloned();
    }
    Ok(())
}
