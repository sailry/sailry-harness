//! Execution-owned attachments are materialized at the model request boundary.
use super::*;
use crate::agent::canonical::{Block, BlockKind, FileRef, Kind, Message, Record, Role};
use base64::Engine as _;
use goose_provider_types::conversation::{
    Conversation,
    message::{DocumentContent, MessageContent},
};
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

/// Historical misses already reported during one turn run.
#[derive(Default)]
pub(super) struct Reported(std::sync::Mutex<std::collections::BTreeSet<AttachmentId>>);

impl Reported {
    /// Returns whether this is the first report of `id`.
    pub(super) fn first(&self, id: AttachmentId) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id)
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
    reported: &Reported,
    conversation: &mut Conversation,
    vision: bool,
    api: ModelApi,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    let report = |id: AttachmentId, reason: &'static str, fault: &Fault| {
        if reported.first(id) {
            omitted(ingress, invocation, id, reason, fault);
        }
    };
    if !vision {
        filter_images(conversation);
    }
    let mut remaining = REQUEST_BYTES;
    for message in conversation.messages_mut() {
        let mut contents = Vec::new();
        for content in std::mem::take(&mut message.content) {
            let MessageContent::Document(document) = &content else {
                if !vision && matches!(&content, MessageContent::Image(_)) {
                    contents.push(MessageContent::text(
                        "Image output omitted because the current model does not support images",
                    ));
                } else {
                    contents.push(content);
                }
                continue;
            };
            if !is_reference(&document.data) {
                contents.push(content);
                continue;
            }
            let reference = id(&document.data)?;
            let source = ingress
                .attachment_source(invocation.turn.id, reference)
                .await?;
            let current = source.current;
            let attachment = match source.attachment {
                Ok(attachment) => attachment,
                Err(fault) if current => return Err(fault),
                Err(fault) => {
                    report(reference, source.reason.unwrap_or("unclassified"), &fault);
                    contents.push(MessageContent::text(unavailable(
                        reference,
                        &document.mime_type,
                    )));
                    continue;
                }
            };
            if document.mime_type != attachment.spec.media_type {
                return Err(Fault::invalid("attachment media type mismatch"));
            }
            let mut materialized = None;
            if attachment.spec.size <= remaining {
                let bytes = match ingress
                    .attachment_bytes(attachment.clone(), stop.clone())
                    .await
                {
                    Ok(bytes) => bytes,
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
                        report(reference, reason, &fault);
                        contents.push(MessageContent::text(unavailable(
                            reference,
                            &attachment.spec.media_type,
                        )));
                        continue;
                    }
                    Err(fault) => return Err(fault),
                };
                materialized = inline(&attachment, bytes, vision, api);
                if materialized.is_some() {
                    remaining -= attachment.spec.size;
                }
            }
            let included = materialized.is_some();
            if let Some(content) = materialized {
                contents.push(content);
            }
            contents.push(MessageContent::text(file(
                &attachment,
                if included { None } else { Some(READ_NOTE) },
            )));
        }
        message.content = contents;
    }
    Ok(())
}

fn inline(
    attachment: &Attachment,
    bytes: Vec<u8>,
    vision: bool,
    api: ModelApi,
) -> Option<MessageContent> {
    let format = file_format::FileFormat::from_bytes(&bytes);
    let mime = format.media_type();
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    if mime.starts_with("image/") && vision {
        return Some(MessageContent::image(data, mime));
    }
    if mime == "application/pdf"
        && matches!(
            api,
            ModelApi::Responses
                | ModelApi::ChatCompletions
                | ModelApi::Anthropic
                | ModelApi::Gemini
                | ModelApi::AzureOpenAi
        )
    {
        return Some(MessageContent::Document(DocumentContent {
            data,
            mime_type: mime.into(),
            name: Some(attachment.spec.name.clone()),
        }));
    }
    if bytes.len() <= 1024 * 1024
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
        if !text.contains('\0') {
            return Some(MessageContent::text(format!(
                "Attachment: {}\n{text}",
                attachment.spec.name
            )));
        }
    }
    None
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

fn filter_images(conversation: &mut Conversation) {
    use rmcp::model::ContentBlock;
    for message in conversation.messages_mut() {
        for content in &mut message.content {
            match content {
                MessageContent::Image(_) => {
                    *content = MessageContent::text(
                        "Image output omitted because the current model does not support images",
                    )
                }
                MessageContent::ToolResponse(response) => {
                    if let Ok(result) = &mut response.tool_result {
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
