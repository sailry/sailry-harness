//! Execution-owned attachments are materialized at the model request boundary.
use super::*;
use crate::agent::canonical::{Block, BlockKind, FileRef, Kind, Message, Record, Role};
use crate::agent::model::{Content, Message as ModelMessage, types::Image};
use crate::store::agent::AttachmentSource;
use base64::Engine as _;
use sailry_protocol::{
    AttachmentId,
    attachment::Attachment,
    conversation::{Input, ModelApi},
};

#[cfg(test)]
mod tests;

const PREFIX: &str = "sailry-attachment://";
const REQUEST_BYTES: u64 = 64 * 1024 * 1024;
const READ_NOTE: &str = "Contents not included; use read_office with source {\"kind\":\"attachment\",\"value\":\"<id>\"} for PDF, Office documents and spreadsheets; use an attachment-capable tool for other files";
const UNAVAILABLE_NOTE: &str = "Attachment unavailable; contents not included";

#[derive(Clone, Default)]
pub(super) struct Cache {
    entries: Arc<tokio::sync::Mutex<std::collections::BTreeMap<AttachmentId, Entry>>>,
}

impl Cache {
    pub(super) async fn promote(&self, ids: impl IntoIterator<Item = AttachmentId>) {
        let mut entries = self.entries.lock().await;
        for id in ids {
            if let Some(entry) = entries.get_mut(&id) {
                entry.source.current = true;
            }
        }
    }
}

struct Entry {
    source: AttachmentSource,
    materialized: Option<Result<Option<Content>, Fault>>,
    reported: bool,
}

impl Entry {
    fn report(
        &mut self,
        ingress: &Ingress,
        invocation: &Invocation,
        id: AttachmentId,
        reason: &'static str,
        fault: &Fault,
    ) {
        if !self.reported {
            omitted(ingress, invocation, id, reason, fault);
            self.reported = true;
        }
    }
}

pub(crate) fn is_reference(uri: &str) -> bool {
    uri.starts_with(PREFIX)
}
pub(crate) fn id(uri: &str) -> Result<AttachmentId, Fault> {
    let value = uri
        .strip_prefix(PREFIX)
        .ok_or_else(|| Fault::invalid("invalid attachment reference"))?;
    let id: AttachmentId = value
        .parse()
        .map_err(|_| Fault::invalid("invalid attachment reference"))?;
    if id.to_string() != value {
        return Err(Fault::invalid("invalid attachment reference"));
    }
    Ok(id)
}

pub(super) async fn input(ingress: &Ingress, invocation: &Invocation) -> Result<Record, Fault> {
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
    input: &Input,
    notice: Option<&str>,
) -> Result<Record, Fault> {
    let mut attachments = Vec::with_capacity(input.attachments.len());
    for id in &input.attachments {
        attachments.push(ingress.attachment(turn, *id).await?);
    }
    render(turn, input, notice, &attachments)
}

/// Builds the canonical user message from already resolved attachments.
pub(crate) fn render(
    turn: TurnId,
    input: &Input,
    notice: Option<&str>,
    attachments: &[Attachment],
) -> Result<Record, Fault> {
    let mut blocks = Vec::new();
    if let Some(text) = notice {
        blocks.push(BlockKind::Text { text: text.into() });
    }
    if !input.references.is_empty() {
        blocks.push(BlockKind::Text{text:format!("User-selected references (plugin and skill targets are explicit capability requests; attachment targets identify attachments of this message by id, and references sharing a label form one selection; other targets are context identifiers, not contents. Use read_file/list_directory for paths and read_session for conversations before describing them):\n{}",serde_json::to_string(&input.references).map_err(|error|Fault::invalid(error.to_string()))?)});
    }
    if !input.text.is_empty() {
        blocks.push(BlockKind::Text {
            text: input.text.clone(),
        });
    }
    for id in &input.attachments {
        let attachment = attachments
            .iter()
            .find(|attachment| attachment.id == *id)
            .ok_or_else(|| Fault::invalid("attachment is not bound to this turn"))?;
        blocks.push(BlockKind::File {
            file: FileRef {
                uri: format!("{PREFIX}{id}"),
                mime: Some(attachment.spec.media_type.clone()),
            },
        });
    }
    let mut record = Record::new(
        turn,
        Kind::Message(Message {
            role: Role::User,
            part_count: blocks.len(),
            content: blocks
                .into_iter()
                .enumerate()
                .map(|(index, kind)| Block { index, kind })
                .collect(),
            ..Message::default()
        }),
    );
    record.provenance.author = "user".into();
    Ok(record)
}

/// Historical references that cannot be read become notes in this request only;
/// the running turn's own input still fails so it can be sent again.
pub(super) async fn hydrate(
    ingress: &Ingress,
    invocation: &Invocation,
    cache: &Cache,
    conversation: &mut [ModelMessage],
    vision: bool,
    api: ModelApi,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    if !vision {
        filter_images(conversation);
    }
    let mut entries = cache.entries.lock().await;
    let mut remaining = REQUEST_BYTES;
    for message in conversation {
        let mut contents = Vec::new();
        for content in std::mem::take(&mut message.content) {
            let Content::Document {
                data_base64,
                media_type,
                ..
            } = &content
            else {
                contents.push(content);
                continue;
            };
            if !is_reference(data_base64) {
                contents.push(content);
                continue;
            }
            let reference = id(data_base64)?;
            let entry = match entries.entry(reference) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => entry.insert(Entry {
                    source: ingress
                        .attachment_source(invocation.turn.id, reference)
                        .await?,
                    materialized: None,
                    reported: false,
                }),
            };
            let current = entry.source.current;
            let attachment = match entry.source.attachment.clone() {
                Ok(attachment) => attachment,
                Err(fault) if current => return Err(fault),
                Err(fault) => {
                    entry.report(
                        ingress,
                        invocation,
                        reference,
                        entry.source.reason.unwrap_or("unclassified"),
                        &fault,
                    );
                    contents.push(Content::Text {
                        text: unavailable(reference, media_type),
                    });
                    continue;
                }
            };
            if *media_type != attachment.spec.media_type {
                return Err(Fault::invalid("attachment media type mismatch"));
            }
            let mut materialized = None;
            if attachment.spec.size <= remaining {
                if entry.materialized.is_none() {
                    entry.materialized = Some(
                        ingress
                            .attachment_bytes(attachment.clone(), stop.clone())
                            .await
                            .map(|bytes| inline(&attachment, bytes, true, ModelApi::Responses)),
                    );
                }
                match entry.materialized.as_ref().unwrap() {
                    Ok(content) => {
                        materialized = content
                            .as_ref()
                            .filter(|content| supported(content, vision, api))
                            .cloned();
                    }
                    Err(fault)
                        if !current
                            && matches!(
                                fault.code,
                                ErrorCode::NotFound | ErrorCode::RevisionConflict
                            ) =>
                    {
                        let reason = if fault.code == ErrorCode::NotFound {
                            "missing_file"
                        } else {
                            "changed_content"
                        };
                        let fault = fault.clone();
                        entry.report(ingress, invocation, reference, reason, &fault);
                        contents.push(Content::Text {
                            text: unavailable(reference, &attachment.spec.media_type),
                        });
                        continue;
                    }
                    Err(fault) => return Err(fault.clone()),
                }
                if materialized.is_some() {
                    remaining -= attachment.spec.size;
                }
            }
            let included = materialized.is_some();
            if let Some(content) = materialized {
                contents.push(content);
            }
            contents.push(Content::Text {
                text: file(&attachment, if included { None } else { Some(READ_NOTE) }),
            });
        }
        message.content = contents;
    }
    Ok(())
}

fn inline(attachment: &Attachment, bytes: Vec<u8>, vision: bool, api: ModelApi) -> Option<Content> {
    let format = file_format::FileFormat::from_bytes(&bytes);
    let mime = format.media_type();
    let content = if mime.starts_with("image/") {
        Content::Image {
            media_type: mime.into(),
            data: Image::Inline {
                data_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
            },
            annotations: None,
        }
    } else if mime == "application/pdf" {
        Content::Document {
            data_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
            media_type: mime.into(),
            name: Some(attachment.spec.name.clone()),
        }
    } else if bytes.len() <= 1024 * 1024
        && (mime.starts_with("text/")
            || matches!(
                mime,
                "application/json"
                    | "application/xml"
                    | "application/yaml"
                    | "application/octet-stream"
            ))
    {
        let text = std::str::from_utf8(&bytes).ok()?;
        if text.contains('\0') {
            return None;
        }
        Content::Text {
            text: format!("Attachment: {}\n{text}", attachment.spec.name),
        }
    } else {
        return None;
    };
    supported(&content, vision, api).then_some(content)
}

fn supported(content: &Content, vision: bool, api: ModelApi) -> bool {
    match content {
        Content::Image { .. } => vision,
        Content::Document { .. } => matches!(
            api,
            ModelApi::Responses
                | ModelApi::ChatCompletions
                | ModelApi::Anthropic
                | ModelApi::Gemini
                | ModelApi::AzureOpenAi
        ),
        _ => true,
    }
}
fn file(attachment: &Attachment, note: Option<&str>) -> String {
    let value = serde_json::json!({"id":attachment.id,"name":attachment.spec.name,"media_type":attachment.spec.media_type,"size":attachment.spec.size});
    match note {
        Some(note) => format!("Attachment reference: {value}\n{note}"),
        None => format!("Attachment reference: {value}"),
    }
}

fn unavailable(id: AttachmentId, media_type: &str) -> String {
    let value = serde_json::json!({"id":id,"media_type":media_type});
    format!("Attachment reference: {value}\n{UNAVAILABLE_NOTE}")
}

fn omitted(
    ingress: &Ingress,
    invocation: &Invocation,
    id: AttachmentId,
    reason: &'static str,
    fault: &Fault,
) {
    let mut entry =
        crate::logging::Entry::new(crate::logging::Area::Run, crate::logging::Phase::Failed);
    entry.context = crate::logging::Context {
        node: Some(ingress.node),
        session: Some(invocation.turn.session),
        turn: Some(invocation.turn.id),
        worktree: Some(invocation.worktree),
        attachment: Some(id),
        ..Default::default()
    };
    entry.operation = Some("attachment");
    entry.stage = Some("hydrate");
    entry.source_kind = Some("history");
    entry.recovery_decision = Some("placeholder");
    entry.reason = Some(reason);
    entry.code = Some(fault.code);
    entry.emit_warn();
}

fn filter_images(conversation: &mut [ModelMessage]) {
    use rmcp::model::ContentBlock;
    for message in conversation {
        for content in &mut message.content {
            match content {
                Content::Image { .. } => {
                    *content = Content::Text {
                        text:
                            "Image output omitted because the current model does not support images"
                                .into(),
                    }
                }
                Content::ToolResult(response) => {
                    if let Ok(result) = &mut response.result {
                        for content in &mut result.content {
                            if matches!(content, ContentBlock::Image(_)) {
                                *content = ContentBlock::text(
                                    "Image output omitted because the current model does not support images",
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
