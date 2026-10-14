//! Content visibility shared by controllers without changing canonical history.
use sailry_protocol::{
    RequestId, TurnId,
    conversation::{Draft, Input, Page, Part, Pending, Queue, Run, SentInput, Status},
};

/// Correlates one local command with its exact durable queued input revision.
pub fn pending(queue: &Queue, request: RequestId) -> Option<&Pending> {
    queue.items.iter().find(|item| item.request == request)
}

/// Canonical user content replaces the claim capture without inventing history entries.
pub fn input(page: &Page, turn: TurnId) -> Option<SentInput> {
    let mut canonical = false;
    let mut message = Input::default();
    let mut attachments = Vec::new();
    for entry in page
        .entries
        .iter()
        .filter(|entry| entry.turn == turn && entry.author == "user")
    {
        canonical = true;
        for part in &entry.parts {
            match part {
                Part::Text(text) => message.text.push_str(text),
                Part::Attachment(attachment) => {
                    message.attachments.push(attachment.id);
                    attachments.push(attachment.clone());
                }
                Part::Reference(reference) => message.references.push(reference.clone()),
                _ => {}
            }
        }
    }
    if canonical {
        Some(SentInput {
            message,
            attachments,
        })
    } else {
        page.runs
            .iter()
            .find(|run| run.turn == turn)
            .and_then(|run| run.input.clone())
    }
}

pub fn visible(run: &Run, page: &Page, drafts: &[Draft]) -> bool {
    run.continuation.is_none()
        || run.status != Status::Continued
        || page
            .entries
            .iter()
            .filter(|entry| entry.turn == run.turn && entry.author != "user")
            .flat_map(|entry| &entry.parts)
            .chain(
                drafts
                    .iter()
                    .filter(|draft| draft.turn == run.turn)
                    .flat_map(|draft| &draft.parts),
            )
            .any(|part| match part {
                Part::Text(text) | Part::Thinking(text) => !text.trim().is_empty(),
                Part::Resource(_) => false,
                _ => true,
            })
}
