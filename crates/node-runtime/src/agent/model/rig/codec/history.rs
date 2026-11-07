use super::{invalid, types};
use base64::Engine as _;
use rig_core::message::{
    AssistantContent, AssistantMessage, CallId, Document, DocumentData, DocumentMediaType,
    DocumentSourceKind, Image, ImageMediaType, Message, MimeType, Origin, ProviderCallId,
    Reasoning, Text, ToolCall, ToolFunction, ToolName, ToolResult, ToolResultContent, UserContent,
};
use rig_core::{
    completion::{Media, Place, ReplayTarget, history::TOOL_IMAGE_OMITTED},
    providers::openai::{
        OpenAIConfig,
        wire::{Chat, DEEPSEEK, OPENAI},
    },
};
use rmcp::model::{ContentBlock, ResourceContents};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use types::{Content, Error, ErrorKind, Role};
use unicode_normalization::UnicodeNormalization;

pub(super) fn messages(
    source: Vec<types::Message>,
    origin: &Origin,
    media: &mut super::super::media::Media,
) -> Result<Vec<Message>, Error> {
    let mut messages = Vec::new();
    let mut calls = BTreeMap::<String, ToolName>::new();
    let chat = (origin.api.as_str() == "openai.chat").then(|| {
        let dialect = if origin.provider == "deepseek" {
            DEEPSEEK
        } else {
            OPENAI
        };
        Chat::new(OpenAIConfig::with_key(&dialect, ""), &origin.model)
    });
    let active = source.iter().rposition(|message| {
        message.metadata.agent_visible
            && message.role == Role::User
            && message
                .content
                .iter()
                .any(|content| !matches!(content, Content::ToolResult(_)))
    });
    for (position, message) in source.into_iter().enumerate() {
        if !message.metadata.agent_visible {
            continue;
        }
        let mut assistant = Vec::new();
        let mut user = Vec::new();
        let keep_thinking = thinking(&message, origin);
        let keep_signature = active.is_none_or(|active| position >= active);
        let mut first_call = true;
        for content in message.content {
            if matches!(content, Content::Error { .. }) {
                continue;
            }
            if !keep_thinking
                && matches!(
                    content,
                    Content::Thinking { .. } | Content::RedactedThinking { .. }
                )
            {
                continue;
            }
            if let Content::ToolResult(result) = content {
                flush(&mut messages, &mut assistant, &mut user, origin);
                let mut result = result_content(result, &calls, origin, media)?;
                let images = if let Some(chat) = &chat {
                    chat_result(&mut result, chat)
                } else {
                    Vec::new()
                };
                messages.push(Message::User {
                    content: vec![UserContent::ToolResult(result)],
                });
                if !images.is_empty() {
                    messages.push(Message::User { content: images });
                }
                continue;
            }
            match message.role {
                Role::Assistant => {
                    let tool = matches!(content, Content::ToolCall(_));
                    let block = assistant_content(content, origin, keep_signature, first_call)?;
                    first_call &= !tool;
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

fn thinking(message: &types::Message, origin: &Origin) -> bool {
    if !matches!(
        origin.api.as_str(),
        "anthropic.messages" | "openai.responses"
    ) {
        return true;
    }
    let Some(inference) = &message.metadata.inference else {
        return origin.api.as_str() != "openai.responses";
    };
    let requested = inference.requested_model.as_str();
    let resolved = inference.resolved_model.as_deref().unwrap_or("");
    (requested.is_empty() && resolved.is_empty())
        || origin.model == requested
        || origin.model == resolved
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

fn assistant_content(
    content: Content,
    origin: &Origin,
    keep_signature: bool,
    first_call: bool,
) -> Result<AssistantContent, Error> {
    Ok(match content {
        Content::Text { text } => {
            let block = AssistantContent::Text(Text::new(text.clone()));
            if origin.api.as_str() == "openai.responses" {
                block.with_native(json!({
                    "type":"message","role":"assistant","status":"completed",
                    "content":[{"type":"output_text","text":text,"annotations":[]}],
                }))
            } else {
                block
            }
        }
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
                    "openai.responses" => {
                        let summary = if text.is_empty() {
                            Vec::new()
                        } else {
                            vec![json!({"type":"summary_text","text":text})]
                        };
                        Some(json!({
                            "type":"reasoning",
                            "summary":summary,
                            "encrypted_content":signature,
                        }))
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
        Content::ToolCall(mut call) => {
            if call.error.is_some() && call.name.is_empty() {
                call.name = "unparseable_tool_call".to_owned();
                call.arguments = json!({});
            }
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
                    if !keep_signature {
                        part.remove("thoughtSignature");
                    }
                    if keep_signature && first_call {
                        part.insert(super::super::wire::SIGNATURE.into(), Value::Bool(true));
                    }
                    part.insert(
                        "functionCall".into(),
                        json!({"id": call.id, "name": call.name, "args": call.arguments}),
                    );
                    block.with_native(Value::Object(part))
                }
                "openai.chat" => {
                    let mut part = call.metadata.unwrap_or_default();
                    part.insert(super::super::wire::CALL_ID.into(), call.id.clone().into());
                    part.insert("id".into(), call.id.into());
                    part.insert("type".into(), "function".into());
                    part.insert(
                        "function".into(),
                        json!({"name": call.name, "arguments": call.arguments.to_string()}),
                    );
                    block.with_native(Value::Object(part))
                }
                "openai.responses" => block.with_native(json!({
                    "type":"function_call","call_id":call.id,"name":call.name,
                    "arguments":call.arguments.to_string(),"status":"completed",
                })),
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
        Content::Error { .. } => return Err(invalid("diagnostics are not model input")),
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
    origin: &Origin,
    media: &mut super::super::media::Media,
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
                        if origin.api.as_str() == "gemini.generate_content" {
                            content.push(media.capture(value.mime_type, value.data));
                        } else {
                            content.push(ToolResultContent::Image(image(
                                value.mime_type,
                                types::Image::Inline {
                                    data_base64: value.data,
                                },
                            )?));
                        }
                    }
                    ContentBlock::Resource(resource) => {
                        if let ResourceContents::BlobResourceContents {
                            blob,
                            mime_type: Some(mime),
                            ..
                        } = &resource.resource
                        {
                            if origin.api.as_str() == "anthropic.messages"
                                && matches!(
                                    mime.as_str(),
                                    "image/png" | "image/jpeg" | "image/gif" | "image/webp"
                                )
                            {
                                content.push(ToolResultContent::Image(image(
                                    mime.clone(),
                                    types::Image::Inline {
                                        data_base64: blob.clone(),
                                    },
                                )?));
                                continue;
                            }
                            if (origin.api.as_str() == "anthropic.messages"
                                && mime == "application/pdf")
                                || (origin.api.as_str() == "gemini.generate_content"
                                    && !mime.is_empty())
                            {
                                content.push(media.capture(mime.clone(), blob.clone()));
                                continue;
                            }
                        }
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

fn chat_result(result: &mut ToolResult, target: &Chat) -> Vec<UserContent> {
    let mut text = Vec::new();
    let mut images = Vec::new();
    let accepts_images = target.accepts(target.model()).user_images;
    for content in std::mem::take(&mut result.content) {
        match content {
            ToolResultContent::Text(content) => text.push(content.text),
            ToolResultContent::Json { value } => text.push(value.to_string()),
            ToolResultContent::Image(image) => {
                let note = if accepts_images
                    && target.encodes(target.model(), Media::Image(&image, Place::User))
                {
                    images.push(UserContent::Image(image));
                    "This tool result included an image that is uploaded in the next message."
                } else {
                    TOOL_IMAGE_OMITTED
                };
                text.push(note.into());
            }
        }
    }
    result.content = vec![ToolResultContent::Text(Text::new(text.join(" ")))];
    images
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_malformed_calls_with_their_result() {
        let call = types::Message::assistant().with_tool_request(
            "invalid",
            Err(rmcp::model::ErrorData::invalid_params(
                "arguments are not JSON",
                None,
            )),
        );
        let result = types::Message::user().with_tool_response(
            "invalid",
            Err(rmcp::model::ErrorData::invalid_params(
                "arguments are not JSON",
                None,
            )),
        );
        let projected = messages(
            vec![call.clone(), result],
            &Origin::new("openai.chat", "openai", "fixture"),
            &mut super::super::super::media::Media::default(),
        )
        .unwrap();
        let Message::Assistant(assistant) = &projected[0] else {
            panic!("assistant message expected")
        };
        let AssistantContent::ToolCall(projected_call) = &assistant.content[0] else {
            panic!("tool call expected")
        };
        assert_eq!(
            projected_call.function.name.as_str(),
            "unparseable_tool_call"
        );
        assert!(projected_call.function.arguments.is_empty());
        let Message::User { content } = &projected[1] else {
            panic!("tool result expected")
        };
        let UserContent::ToolResult(result) = &content[0] else {
            panic!("tool result expected")
        };
        assert_eq!(result.call, projected_call.id);
        assert_eq!(result.name, projected_call.function.name);
        let Content::ToolCall(call) = &call.content[0] else {
            panic!("canonical tool call expected")
        };
        assert!(call.name.is_empty());
        assert!(call.error.is_some());
    }
}
