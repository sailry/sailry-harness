//! Content visibility shared by controllers without changing canonical history.
use sailry_protocol::{
    RequestId, TurnId,
    conversation::{Draft, Input, Page, Part, Pending, Queue, Run, SentInput, Status},
};

/// Correlates one local command with its exact durable queued input revision.
pub fn pending(queue: &Queue, request: RequestId) -> Option<&Pending> {
    queue
        .items
        .iter()
        .find(|item| item.request == Some(request))
}

/// Pending guidance follows queue order until its canonical user entry arrives.
pub fn guided(page: &Page) -> Vec<&Run> {
    page.queue
        .items
        .iter()
        .filter(|item| item.guidance.is_some())
        .filter_map(|item| {
            page.runs.iter().find(|run| {
                run.turn == item.turn
                    && run.status == Status::Queued
                    && run.input.is_some()
                    && !page
                        .entries
                        .iter()
                        .any(|entry| entry.turn == run.turn && entry.author == "user")
            })
        })
        .collect()
}

/// Joined inputs share their response's row while retaining their source turns.
pub fn owner(page: &Page, turn: TurnId) -> TurnId {
    page.runs
        .iter()
        .find(|run| run.turn == turn)
        .and_then(|run| run.joined)
        .filter(|owner| page.runs.iter().any(|run| run.turn == *owner))
        .unwrap_or(turn)
}

/// Canonical user order supplies the input order within one response.
pub fn group(page: &Page, turn: TurnId) -> Vec<&Run> {
    let mut runs: Vec<_> = page
        .runs
        .iter()
        .filter(|run| run.turn == turn || run.joined == Some(turn))
        .collect();
    runs.sort_by_key(|run| {
        let sequence = page
            .entries
            .iter()
            .find(|entry| entry.turn == run.turn && entry.author == "user")
            .map(|entry| entry.sequence);
        (
            run.turn != turn,
            sequence.is_none(),
            sequence.unwrap_or(run.sequence),
        )
    });
    runs
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct InputView {
    #[serde(flatten)]
    pub input: SentInput,
    pub canonical: bool,
}

/// Canonical user entries replace the admitted capture without inventing history.
pub fn inputs(page: &Page, turn: TurnId) -> Vec<InputView> {
    if page.runs.iter().any(|run| run.turn == turn && run.notice) {
        return Vec::new();
    }
    let canonical: Vec<_> = page
        .entries
        .iter()
        .filter(|entry| entry.turn == turn && entry.author == "user")
        .map(|entry| {
            let mut message = Input::default();
            let mut attachments = Vec::new();
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
            InputView {
                input: SentInput {
                    message,
                    attachments,
                },
                canonical: true,
            }
        })
        .collect();
    if !canonical.is_empty() {
        return canonical;
    }
    page.runs
        .iter()
        .find(|run| run.turn == turn)
        .and_then(|run| run.input.clone())
        .map(|input| {
            vec![InputView {
                input,
                canonical: false,
            }]
        })
        .unwrap_or_default()
}

pub fn input(page: &Page, turn: TurnId) -> Option<SentInput> {
    let mut selected = inputs(page, turn).into_iter();
    let mut input = selected.next()?.input;
    for next in selected {
        input.message.text.push_str(&next.input.message.text);
        input
            .message
            .attachments
            .extend(next.input.message.attachments);
        input
            .message
            .references
            .extend(next.input.message.references);
        input.attachments.extend(next.input.attachments);
    }
    Some(input)
}

pub fn visible(run: &Run, page: &Page, drafts: &[Draft]) -> bool {
    if owner(page, run.turn) != run.turn {
        return false;
    }
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
                Part::Resource(value) => {
                    sailry_protocol::conversation::compaction::Failure::from_resource(value)
                        .is_some()
                }
                _ => true,
            })
}
