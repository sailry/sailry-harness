use super::wire::Part;
use super::*;

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
                let status = match annotations
                    .as_ref()
                    .and_then(|value| value.get("adk.tool_result.status"))
                    .and_then(Value::as_str)
                {
                    Some("failed") => ToolStatus::Error,
                    Some("pending") => ToolStatus::Pending,
                    Some("cancelled") => ToolStatus::Cancelled,
                    Some("outcome_unknown") => ToolStatus::Unknown,
                    _ => ToolStatus::Success,
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
                value: blobs::capture(db, resource.clone()).await?,
            },
        };
        blocks.push(Block { index, kind });
    }
    Ok(blocks)
}
