//! Materializes execution-owned references only at the provider boundary.
use super::*;
use adk_core::{Content, Llm, LlmRequest, LlmResponseStream, Part, SchemaAdapter};
use async_trait::async_trait;
use sailry_protocol::{AttachmentId, attachment::Attachment, conversation::ModelApi};

mod media;
use media::inline;

const PREFIX: &str = "sailry-attachment://";
const REQUEST_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) fn is_reference(uri: &str) -> bool {
    uri.starts_with(PREFIX)
}

pub(crate) fn id(uri: &str) -> Result<AttachmentId, Fault> {
    let value = uri
        .strip_prefix(PREFIX)
        .ok_or_else(|| invalid("invalid attachment reference"))?;
    let id: AttachmentId = value
        .parse()
        .map_err(|_| invalid("invalid attachment reference"))?;
    if id.to_string() != value {
        return Err(invalid("invalid attachment reference"));
    }
    Ok(id)
}

pub(super) async fn input(ingress: &Ingress, invocation: &Invocation) -> Result<Content, Fault> {
    let mut content = Content::new("user");
    if !invocation.message.references.is_empty() {
        // The canonical user event retains this context; projection uses its typed metadata.
        content = content.with_text(format!(
            "User-selected references (plugin and skill targets are explicit capability requests; other targets are context identifiers, not contents. Use read_file/list_directory for paths and read_session for conversations before describing them):\n{}",
            serde_json::to_string(&invocation.message.references)
                .map_err(|error| invalid(&error.to_string()))?
        ));
    }
    if !invocation.message.text.is_empty() {
        content = content.with_text(invocation.message.text.clone());
    }
    for id in &invocation.message.attachments {
        let attachment = ingress.attachment(invocation.turn.id, *id).await?;
        content.parts.push(Part::FileData {
            mime_type: attachment.spec.media_type,
            file_uri: format!("{PREFIX}{id}"),
            annotations: None,
        });
    }
    Ok(content)
}

pub(super) struct Model {
    pub(super) inner: Arc<dyn Llm>,
    pub(super) ingress: Arc<Ingress>,
    pub(super) turn: TurnId,
    pub(super) vision: bool,
    pub(super) api: Option<ModelApi>,
    pub(super) tools: bool,
    pub(super) stop: CancellationToken,
}

impl Model {
    async fn hydrate(&self, request: &mut LlmRequest) -> Result<(), Fault> {
        if !self.vision {
            filter_images(&mut request.contents);
        }
        let mut remaining = REQUEST_BYTES;
        for content in &mut request.contents {
            let mut references = Vec::new();
            for part in &mut content.parts {
                let Part::FileData {
                    file_uri,
                    mime_type,
                    ..
                } = part
                else {
                    continue;
                };
                if !is_reference(file_uri) {
                    continue;
                }
                let attachment = self.ingress.attachment(self.turn, id(file_uri)?).await?;
                if attachment.spec.media_type != *mime_type {
                    return Err(invalid("attachment media type mismatch"));
                }
                let native = if attachment.spec.size <= adk_core::MAX_INLINE_DATA_SIZE as u64
                    && attachment.spec.size <= remaining
                {
                    let bytes = self
                        .ingress
                        .attachment_bytes(attachment.clone(), self.stop.clone())
                        .await?;
                    inline(&attachment, bytes, self.vision, self.api)
                } else {
                    None
                };
                *part = match native {
                    Some(native) => {
                        remaining -= attachment.spec.size;
                        if self.tools {
                            references.push(file(&attachment, true));
                        }
                        native
                    }
                    None if self.tools => file(&attachment, false),
                    None => {
                        return Err(invalid(
                            "attachment requires tools or a supported native model input",
                        ));
                    }
                };
            }
            content.parts.extend(references);
        }
        Ok(())
    }
}

fn filter_images(contents: &mut [Content]) {
    for part in contents.iter_mut().flat_map(|content| &mut content.parts) {
        match part {
            Part::InlineData { mime_type, .. } if mime_type.starts_with("image/") => {
                *part = Part::Text {
                    text: "Image output omitted because the current model does not support images"
                        .into(),
                };
            }
            Part::FunctionResponse {
                function_response, ..
            } => {
                function_response
                    .inline_data
                    .retain(|data| !data.mime_type.starts_with("image/"));
            }
            _ => {}
        }
    }
}

fn file(attachment: &Attachment, included: bool) -> Part {
    let metadata = serde_json::json!({
        "id": attachment.id,
        "name": attachment.spec.name,
        "media_type": attachment.spec.media_type,
        "size": attachment.spec.size,
    });
    let note = if included {
        ""
    } else {
        "\nContents not included; an attachment-capable tool is required to inspect the file"
    };
    Part::Text {
        text: format!("Attachment file: {metadata}{note}"),
    }
}

#[async_trait]
impl Llm for Model {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn schema_adapter(&self) -> &dyn SchemaAdapter {
        self.inner.schema_adapter()
    }
    fn uses_interactions_api(&self) -> bool {
        self.inner.uses_interactions_api()
    }

    async fn generate_content(
        &self,
        mut request: LlmRequest,
        stream: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        model::history::coalesce(&mut request.contents);
        self.hydrate(&mut request).await.map_err(|fault| {
            adk_core::AdkError::model("attachment input failed").with_source(fault)
        })?;
        model::timing::generate(&self.inner, request, stream).await
    }
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}

#[cfg(test)]
mod tests;
