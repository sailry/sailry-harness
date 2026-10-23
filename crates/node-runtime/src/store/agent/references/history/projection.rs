//! Model reads retain public history identities, continuations and explicit content omissions.
use super::*;
use serde::Serialize;
use std::collections::BTreeSet;

const MAX_BYTES: usize = 128 * 1024;
const PART_BYTES: usize = 8 * 1024;
const ENTRY_BYTES: usize = 16 * 1024;
const INPUT_BYTES: usize = 8 * 1024;

#[derive(Serialize)]
struct Turn {
    turn: TurnId,
    worktree: WorktreeId,
    sequence: u64,
    kind: RunKind,
    status: Status,
    error: Option<Fault>,
    started_ms: Option<i64>,
    finished_ms: Option<i64>,
    input: Option<SentInput>,
}

#[derive(Default, Serialize)]
struct Truncation {
    truncated: bool,
    truncated_entries: BTreeSet<String>,
    truncated_inputs: BTreeSet<TurnId>,
}

#[derive(Serialize)]
struct Page {
    session: SessionId,
    revision: u64,
    entries: Vec<Entry>,
    runs: Vec<Turn>,
    next_before: Option<TurnId>,
}

#[derive(Serialize)]
struct Conversation {
    page: Page,
    missing: BTreeSet<TurnId>,
    #[serde(flatten)]
    truncation: Truncation,
}

#[derive(Serialize)]
struct Continuation {
    revision: u64,
    run: Turn,
    entries: Vec<Entry>,
    next_before: Option<u64>,
    #[serde(flatten)]
    truncation: Truncation,
}

pub(super) fn conversation(history: History) -> Result<Value, Fault> {
    let mut truncation = Truncation::default();
    let mut response = Conversation {
        missing: history.missing.into_iter().collect(),
        page: Page {
            session: history.page.session,
            revision: history.page.revision,
            entries: history
                .page
                .entries
                .into_iter()
                .map(|entry| bounded_entry(entry, &mut truncation))
                .collect::<Result<_, _>>()?,
            runs: history
                .page
                .runs
                .into_iter()
                .map(|run| bounded_run(run, &mut truncation))
                .collect(),
            next_before: history.page.next_before,
        },
        truncation,
    };
    while serde_json::to_vec(&response).map_err(storage_error)?.len() > MAX_BYTES {
        response.truncation.truncated = true;
        if response.page.entries.len() > 1 {
            let entry = response.page.entries.remove(0);
            response.missing.insert(entry.turn);
            response.truncation.truncated_entries.remove(&entry.id);
        } else if response.page.runs.len() > 1 {
            let run = response.page.runs.remove(0);
            response.page.entries.retain(|entry| entry.turn != run.turn);
            response.missing.remove(&run.turn);
            response.truncation.truncated_inputs.remove(&run.turn);
            response.page.next_before = Some(response.page.runs[0].turn);
        } else {
            return Err(Fault::invalid(
                "history metadata exceeds the model output budget",
            ));
        }
    }
    serde_json::to_value(response).map_err(storage_error)
}

pub(super) fn turn(history: TurnHistory) -> Result<Value, Fault> {
    let mut truncation = Truncation::default();
    let mut response = Continuation {
        revision: history.revision,
        run: bounded_run(history.run, &mut truncation),
        entries: history
            .entries
            .into_iter()
            .map(|entry| bounded_entry(entry, &mut truncation))
            .collect::<Result<_, _>>()?,
        next_before: history.next_before,
        truncation,
    };
    while serde_json::to_vec(&response).map_err(storage_error)?.len() > MAX_BYTES {
        if response.entries.len() <= 1 {
            return Err(Fault::invalid(
                "turn metadata exceeds the model output budget",
            ));
        }
        let entry = response.entries.remove(0);
        response.truncation.truncated_entries.remove(&entry.id);
        response.truncation.truncated = true;
        response.next_before = Some(response.entries[0].sequence);
    }
    serde_json::to_value(response).map_err(storage_error)
}

fn bounded_run(mut run: Run, truncation: &mut Truncation) -> Turn {
    if let Some(error) = &mut run.error {
        truncation.truncated |= trim(&mut error.message, 2048);
    }
    if let Some(input) = &mut run.input {
        let mut trimmed = trim(&mut input.message.text, INPUT_BYTES);
        for length in [
            input.message.references.len(),
            input.message.attachments.len(),
            input.attachments.len(),
        ] {
            trimmed |= length > 16;
        }
        input.message.references.truncate(16);
        input.message.attachments.truncate(16);
        input.attachments.truncate(16);
        if trimmed {
            truncation.truncated = true;
            truncation.truncated_inputs.insert(run.turn);
        }
    }
    Turn {
        turn: run.turn,
        worktree: run.worktree,
        sequence: run.sequence,
        kind: run.kind,
        status: run.status,
        error: run.error,
        started_ms: run.started_ms,
        finished_ms: run.finished_ms,
        input: run.input,
    }
}

fn bounded_entry(mut entry: Entry, truncation: &mut Truncation) -> Result<Entry, Fault> {
    let mut parts = Vec::new();
    let mut bytes = 0;
    let mut trimmed = false;
    for mut part in entry.parts {
        let encoded = serde_json::to_string(&part).map_err(storage_error)?;
        if encoded.len() > PART_BYTES {
            trimmed = true;
            match &mut part {
                Part::Text(text) | Part::Thinking(text) => {
                    trim(text, PART_BYTES / 2);
                }
                Part::ToolResult { result, images, .. } => {
                    *result = serde_json::json!({"truncated":true,"preview":prefix(&serde_json::to_string(result).map_err(storage_error)?, PART_BYTES / 2)});
                    images.clear();
                }
                _ => {
                    part = Part::Resource(serde_json::json!({
                        "type":"truncated_history_part",
                        "preview":prefix(&encoded, PART_BYTES / 2),
                    }));
                }
            }
        }
        let size = serde_json::to_vec(&part).map_err(storage_error)?.len();
        if bytes + size > ENTRY_BYTES || parts.len() == 32 {
            trimmed = true;
            break;
        }
        bytes += size;
        parts.push(part);
    }
    entry.parts = parts;
    if let Some(html) = &mut entry.search_suggestions {
        trimmed |= trim(html, PART_BYTES / 2);
    }
    if entry.citations.len() > 16 {
        entry.citations.truncate(16);
        trimmed = true;
    }
    if trimmed {
        truncation.truncated = true;
        truncation.truncated_entries.insert(entry.id.clone());
    }
    Ok(entry)
}

fn prefix(text: &str, limit: usize) -> &str {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn trim(text: &mut String, limit: usize) -> bool {
    let end = prefix(text, limit).len();
    let trimmed = end < text.len();
    text.truncate(end);
    trimmed
}
