use super::{invalid, types};
use base64::Engine as _;
use rig_core::message::{
    AssistantContent, AssistantMessage, CallId, Document, DocumentData, DocumentMediaType,
    DocumentSourceKind, Image, ImageMediaType, Message, MimeType, Origin, ProviderCallId,
    Reasoning, Text, ToolCall, ToolFunction, ToolName, ToolResult, ToolResultContent, UserContent,
};
use rmcp::model::{ContentBlock, ResourceContents};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use types::{Content, Error, ErrorKind, Role};
use unicode_normalization::UnicodeNormalization;

pub(super) fn messages(
    source: Vec<types::Message>,
    origin: &Origin,
) -> Result<Vec<Message>, Error> {
    let mut messages = Vec::new();
    let mut calls = BTreeMap::<String, ToolName>::new();
    for message in source {
        let mut assistant = Vec::new();
        let mut user = Vec::new();
        for content in message.content {
            if let Content::ToolResult(result) = content {
                flush(&mut messages, &mut assistant, &mut user, origin);
                let result = result_content(result, &calls)?;
                messages.push(Message::User {
                    content: vec![UserContent::ToolResult(result)],
                });
                continue;
            }
            match message.role {
                Role::Assistant => {
                    let block = assistant_content(content, origin)?;
                    if let AssistantContent::ToolCall(call) = &block {
                        calls.insert(call.id.to_string(), call.function.name.clone());
                    }
                    assistant.push(block);
                }
                Role::User => user.push(user_content(content)?),
            }
        }
        flush(&mut messages, &mut assistant, &mut user, origin);
    }
    Ok(messages)
}

fn flush(
    messages: &mut Vec<Message>,
    assistant: &mut Vec<AssistantContent>,
    user: &mut Vec<UserContent>,
    origin: &Origin,
) {
    if !assistant.is_empty() {
        messages.push(Message::Assistant(
            AssistantMessage::new(std::mem::take(assistant)).with_origin(origin.clone()),
        ));
    }
    if !user.is_empty() {
        messages.push(Message::User {
            content: std::mem::take(user),
        });
    }
}

fn assistant_content(content: Content, origin: &Origin) -> Result<AssistantContent, Error> {
    Ok(match content {
        Content::Text { text } => AssistantContent::Text(Text::new(text)),
        Content::Thinking { text, signature } => {
            let mut block = AssistantContent::Reasoning(Reasoning::new(text.clone()));
            if let Some(signature) = signature.filter(|signature| !signature.is_empty()) {
                let native = match origin.api.as_str() {
                    "anthropic.messages" => {
                        Some(json!({"type": "thinking", "thinking": text, "signature": signature}))
                    }
                    "gemini.generate_content" => {
                        Some(json!({"text": text, "thought": true, "thoughtSignature": signature}))
                    }
                    _ => None,
                };
                if let Some(native) = native {
                    block = block.with_native(native);
                }
            }
            block
        }
        Content::RedactedThinking { data } => {
            if origin.api.as_str() != "anthropic.messages" {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "redacted reasoning requires Anthropic Messages",
                ));
            }
            AssistantContent::Reasoning(Reasoning {
                redacted: true,
                ..Reasoning::default()
            })
            .with_native(json!({"type": "redacted_thinking", "data": data}))
        }
        Content::ToolCall(call) => {
            let name = ToolName::new(call.name.clone()).map_err(invalid)?;
            let id = identity(&call.id)?;
            let mut function = ToolFunction::new(name, call.arguments.clone());
            if call.error.is_some() && function.invalid_arguments.is_none() {
                function.invalid_arguments = Some(call.arguments.to_string());
            }
            let block = AssistantContent::ToolCall(ToolCall::new(id, function));
            match origin.api.as_str() {
                "gemini.generate_content" => {
                    let mut part = call.metadata.unwrap_or_default();
                    part.insert(
                        "functionCall".into(),
                        json!({"id": call.id, "name": call.name, "args": call.arguments}),
                    );
                    block.with_native(Value::Object(part))
                }
                "openai.chat" => {
                    let mut part = call.metadata.unwrap_or_default();
                    part.insert("id".into(), call.id.into());
                    part.insert("type".into(), "function".into());
                    part.insert(
                        "function".into(),
                        json!({"name": call.name, "arguments": call.arguments.to_string()}),
                    );
                    block.with_native(Value::Object(part))
                }
                _ => block,
            }
        }
        Content::Image {
            media_type, data, ..
        } => AssistantContent::Image(image(media_type, data)?),
        Content::Document { .. } => {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "assistant document input is unsupported",
            ));
        }
        Content::ToolResult(_) => return Err(invalid("tool results require user content")),
    })
}

fn user_content(content: Content) -> Result<UserContent, Error> {
    Ok(match content {
        Content::Text { text } => UserContent::Text(Text::new(text)),
        Content::Image {
            media_type, data, ..
        } => UserContent::Image(image(media_type, data)?),
        Content::Document {
            media_type,
            name,
            data_base64,
        } => {
            let media_type = DocumentMediaType::from_mime_type(&media_type)
                .ok_or_else(|| invalid("document media type is unsupported"))?;
            UserContent::Document(Document {
                data: DocumentData::File(DocumentSourceKind::Base64(data_base64)),
                media_type: Some(media_type),
                additional_params: name.map(|name| json!({"title": name})),
            })
        }
        _ => return Err(invalid("user messages contain unsupported model content")),
    })
}

fn image(media_type: String, source: types::Image) -> Result<Image, Error> {
    let media_type = ImageMediaType::from_mime_type(&media_type)
        .ok_or_else(|| invalid("image media type is unsupported"))?;
    Ok(Image {
        data: match source {
            types::Image::Inline { data_base64 } => DocumentSourceKind::Base64(data_base64),
            types::Image::Url { url } => DocumentSourceKind::Url(url),
        },
        media_type: Some(media_type),
        ..Image::default()
    })
}

fn identity(value: &str) -> Result<CallId, Error> {
    ProviderCallId::new(value)
        .map(CallId::from)
        .ok_or_else(|| invalid("tool call identity is missing"))
}

fn result_content(
    source: types::ToolResult,
    calls: &BTreeMap<String, ToolName>,
) -> Result<ToolResult, Error> {
    let name = if source.name.is_empty() {
        calls
            .get(&source.id)
            .cloned()
            .ok_or_else(|| invalid("tool result has no matching call name"))?
    } else {
        ToolName::new(source.name).map_err(invalid)?
    };
    let mut content = Vec::new();
    let is_error = match source.result {
        Ok(result) => {
            for part in result.content {
                match part {
                    ContentBlock::Text(text) => {
                        content.push(ToolResultContent::Text(Text::new(text.text)));
                    }
                    ContentBlock::Image(value) => {
                        content.push(ToolResultContent::Image(image(
                            value.mime_type,
                            types::Image::Inline {
                                data_base64: value.data,
                            },
                        )?));
                    }
                    ContentBlock::Resource(resource) => {
                        content.push(ToolResultContent::Text(Text::new(resource_text(
                            resource.resource,
                        ))));
                    }
                    _ => {}
                }
            }
            if content.is_empty()
                && let Some(value) = result.structured_content
            {
                content.push(ToolResultContent::Json { value });
            }
            result.is_error.unwrap_or(false)
        }
        Err(error) => {
            content.push(ToolResultContent::Text(Text::new(format!(
                "The tool call returned the following error:\n{error}"
            ))));
            true
        }
    };
    if content.is_empty() {
        content.push(ToolResultContent::Text(Text::new("")));
    }
    Ok(ToolResult {
        call: identity(&source.id)?,
        name,
        content,
        is_error,
    })
}

// Resource projection preserves Goose 14741e9a's mcp_utils behavior (Apache-2.0).
fn resource_text(resource: ResourceContents) -> String {
    let text = match resource {
        ResourceContents::TextResourceContents { text, .. } => text,
        ResourceContents::BlobResourceContents {
            blob, mime_type, ..
        } => match base64::engine::general_purpose::STANDARD.decode(&blob) {
            Ok(bytes) => {
                let length = bytes.len();
                String::from_utf8(bytes).unwrap_or_else(|_| {
                    format!(
                        "[Binary content ({}) - {length} bytes]",
                        mime_type.as_deref().unwrap_or("application/octet-stream")
                    )
                })
            }
            Err(_) => blob,
        },
        _ => String::new(),
    };
    text.nfc()
        .filter(|character| !('\u{e0000}'..='\u{e007f}').contains(character))
        .collect()
}
