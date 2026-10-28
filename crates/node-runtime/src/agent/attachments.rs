//! Materializes execution-owned references only at the provider boundary.
use super::*;
use adk_core::{Content, Llm, LlmRequest, LlmResponseStream, Part, SchemaAdapter};
use async_trait::async_trait;
use sailry_protocol::{
    AttachmentId, Authentication,
    attachment::Attachment,
    conversation::{ModelApi, Provider},
};

mod media;
use media::inline;

const PREFIX: &str = "sailry-attachment://";
const REQUEST_BYTES: u64 = 64 * 1024 * 1024;
const READ_NOTE: &str = "Contents not included; use read_office with source {\"kind\":\"attachment\",\"value\":\"<id>\"} for PDF, Office documents and spreadsheets; use an attachment-capable tool for other files";
const UNAVAILABLE_NOTE: &str =
    "Contents not included; the current model cannot read this attachment";
const IMAGE_NOTE: &str = "This model does not support images. The original attachment is retained.";
const BUDGET_NOTE: &str = "Contents not included because they exceed this handoff request's input budget. Preserve the attachment reference without claiming its contents were inspected.";

pub(super) fn api(provider: &Provider, effective: ModelApi) -> Option<ModelApi> {
    if effective == ModelApi::Plugin {
        return provider
            .binding
            .as_ref()
            .and_then(|binding| binding.adapter.as_ref())
            .filter(|adapter| adapter.capabilities.files)
            .map(|_| ModelApi::Plugin);
    }
    if provider.authentication != Authentication::ApiKey {
        return None;
    }
    // Image support does not establish inline document support. Compatible
    // OpenAI endpoints may accept only uploaded file IDs or public URLs, so
    // private documents use the existing attachment-reading tools instead.
    // https://developers.openai.com/api/docs/guides/file-inputs
    // https://docs.x.ai/developers/model-capabilities/files/chat-with-files
    if matches!(effective, ModelApi::ChatCompletions | ModelApi::Responses)
        && provider.endpoint.trim_end_matches('/') != "https://api.openai.com/v1"
    {
        return None;
    }
    Some(effective)
}

pub(crate) fn is_reference(uri: &str) -> bool {
    uri.starts_with(PREFIX)
}

pub(crate) fn id(uri: &str) -> Result<AttachmentId, Fault> {
    let value = uri
        .strip_prefix(PREFIX)
        .ok_or_else(|| sailry_protocol::Fault::invalid("invalid attachment reference"))?;
    let id: AttachmentId = value
        .parse()
        .map_err(|_| sailry_protocol::Fault::invalid("invalid attachment reference"))?;
    if id.to_string() != value {
        return Err(sailry_protocol::Fault::invalid(
            "invalid attachment reference",
        ));
    }
    Ok(id)
}

pub(super) async fn input(ingress: &Ingress, invocation: &Invocation) -> Result<Content, Fault> {
    message(
        ingress,
        invocation.turn.id,
        &invocation.message,
        invocation.notice_input.as_deref(),
    )
    .await
}

pub(super) async fn message(
    ingress: &Ingress,
    turn: TurnId,
    message: &sailry_protocol::conversation::Input,
    notice: Option<&str>,
) -> Result<Content, Fault> {
    let mut content = Content::new("user");
    if let Some(notice) = notice {
        content = content.with_text(notice);
    }
    if !message.references.is_empty() {
        // The canonical user event retains this context; projection uses its typed metadata.
        content = content.with_text(format!(
            "User-selected references (plugin and skill targets are explicit capability requests; other targets are context identifiers, not contents. Use read_file/list_directory for paths and read_session for conversations before describing them):\n{}",
            serde_json::to_string(&message.references)
                .map_err(|error| sailry_protocol::Fault::invalid(error.to_string()))?
        ));
    }
    if !message.text.is_empty() {
        content = content.with_text(message.text.clone());
    }
    for id in &message.attachments {
        let attachment = ingress.attachment(turn, *id).await?;
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
    pub(super) window: Option<model::history::Window>,
    pub(super) tools: bool,
    pub(super) stop: CancellationToken,
    pub(super) identity: crate::logging::Context,
}

impl Model {
    async fn hydrate(&self, request: &mut LlmRequest) -> Result<(), Fault> {
        if !self.vision {
            filter_images(&mut request.contents);
        }
        let mut remaining = REQUEST_BYTES;
        let summary = model::is_summary();
        let mut fallbacks = Vec::new();
        let mut changed = false;
        for (content_index, content) in request.contents.iter_mut().enumerate() {
            let mut references = Vec::new();
            for (part_index, part) in content.parts.iter_mut().enumerate() {
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
                changed = true;
                let reference = id(file_uri)?;
                let (attachment, current) =
                    self.ingress.attachment_source(self.turn, reference).await?;
                if attachment.spec.media_type != *mime_type {
                    return Err(sailry_protocol::Fault::invalid(
                        "attachment media type mismatch",
                    ));
                }
                let native = if attachment.spec.size <= adk_core::MAX_INLINE_DATA_SIZE as u64
                    && attachment.spec.size <= remaining
                {
                    let bytes = self
                        .ingress
                        .attachment_bytes(attachment.clone(), self.stop.clone())
                        .await?;
                    if !self.vision
                        && file_format::FileFormat::from_bytes(&bytes)
                            .media_type()
                            .starts_with("image/")
                    {
                        *part = file(&attachment, Some(IMAGE_NOTE));
                        continue;
                    }
                    inline(&attachment, bytes, self.vision, self.api)
                } else {
                    None
                };
                *part = match native {
                    Some(native) => {
                        remaining -= attachment.spec.size;
                        if summary {
                            let allowance = match &native {
                                Part::InlineData {
                                    mime_type, data, ..
                                } if !mime_type.starts_with("image/") => {
                                    (data.len() as u64).div_ceil(3)
                                }
                                _ => 0,
                            };
                            fallbacks.push((
                                content_index,
                                part_index,
                                file(&attachment, Some(BUDGET_NOTE)),
                                allowance,
                            ));
                        }
                        if self.tools {
                            references.push(file(&attachment, None));
                        }
                        native
                    }
                    None if self.tools => file(&attachment, Some(READ_NOTE)),
                    // A previous unsupported input must not make every later
                    // plain-text turn fail. Keep its identity and state the
                    // missing contents without pretending they were read.
                    None if !current => file(&attachment, Some(UNAVAILABLE_NOTE)),
                    None => {
                        return Err(sailry_protocol::Fault::invalid(
                            "attachment requires tools or a supported native model input",
                        ));
                    }
                };
            }
            content.parts.extend(references);
        }
        if changed {
            // Hydration can expand a small reference into real text or media.
            // The shared model boundary must size that changed request again.
            model::history::clear_prepared(request);
        }
        if let Some(window) = self.window.filter(|_| summary && !fallbacks.is_empty()) {
            let capacity = window.capacity(request);
            // Native document/media tokenization is provider-specific. Apply
            // this conservative byte allowance only to summary replacement.
            let mut tokens = compaction::estimate::request(request)
                .map_err(model::error)?
                .saturating_add(fallbacks.iter().map(|(_, _, _, allowance)| allowance).sum());
            for (content, part, fallback, allowance) in fallbacks {
                if tokens <= capacity {
                    break;
                }
                let value = &mut request.contents[content].parts[part];
                tokens = tokens
                    .saturating_sub(compaction::estimate::part(value))
                    .saturating_sub(allowance)
                    .saturating_add(compaction::estimate::part(&fallback));
                *value = fallback;
            }
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

fn file(attachment: &Attachment, note: Option<&str>) -> Part {
    let metadata = serde_json::json!({
        "id": attachment.id,
        "name": attachment.spec.name,
        "media_type": attachment.spec.media_type,
        "size": attachment.spec.size,
    });
    Part::Text {
        text: match note {
            Some(note) => format!("Attachment file: {metadata}\n{note}"),
            None => format!("Attachment file: {metadata}"),
        },
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

    fn max_cost_micro_usd(&self, request: &adk_core::LlmRequest) -> Option<u64> {
        self.inner.max_cost_micro_usd(request)
    }

    async fn generate_content(
        &self,
        mut request: LlmRequest,
        stream: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        self.hydrate(&mut request).await.map_err(|fault| {
            adk_core::AdkError::model("attachment input failed").with_source(fault)
        })?;
        model::timing::generate(
            &self.inner,
            request,
            stream,
            "model_request",
            &self.identity,
            self.stop.clone(),
        )
        .await
    }
}

#[cfg(test)]
mod tests;
