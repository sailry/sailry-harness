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
    entries: Vec<Value>,
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
    entries: Vec<Value>,
    next_before: Option<u64>,
    #[serde(flatten)]
    truncation: Truncation,
}

pub(super) async fn conversation(
    db: &Connection,
    session: SessionId,
    history: History,
) -> Result<Value, Fault> {
    let mut truncation = Truncation::default();
    let mut entries = Vec::new();
    for entry in history.page.entries {
        entries.push(bounded_entry(db, session, entry, &mut truncation).await?);
    }
    let mut response = Conversation {
        missing: history.missing.into_iter().collect(),
        page: Page {
            session: history.page.session,
            revision: history.page.revision,
            entries,
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
            response
                .missing
                .insert(serde_json::from_value(entry["turn"].clone()).map_err(storage_error)?);
            response
                .truncation
                .truncated_entries
                .remove(entry["id"].as_str().unwrap_or_default());
        } else if response.page.runs.len() > 1 {
            let run = response.page.runs.remove(0);
            response
                .page
                .entries
                .retain(|entry| entry["turn"] != run.turn.to_string());
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

pub(super) async fn turn(
    db: &Connection,
    session: SessionId,
    history: TurnHistory,
) -> Result<Value, Fault> {
    let mut truncation = Truncation::default();
    let mut entries = Vec::new();
    for entry in history.entries {
        entries.push(bounded_entry(db, session, entry, &mut truncation).await?);
    }
    let mut response = Continuation {
        revision: history.revision,
        run: bounded_run(history.run, &mut truncation),
        entries,
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
        response
            .truncation
            .truncated_entries
            .remove(entry["id"].as_str().unwrap_or_default());
        response.truncation.truncated = true;
        response.next_before = response.entries[0]["sequence"].as_u64();
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

async fn bounded_entry(
    db: &Connection,
    session: SessionId,
    mut entry: Entry,
    truncation: &mut Truncation,
) -> Result<Value, Fault> {
    let mut parts = Vec::new();
    let mut bytes = 0;
    let mut trimmed = false;
    let source = if entry
        .parts
        .iter()
        .any(|part| matches!(part, Part::ToolCall { .. } | Part::ToolResult { .. }))
    {
        Some(history::canonical::event(db, session, entry.turn, &entry.id).await?)
    } else {
        None
    };
    for (index, part) in entry.parts.iter().enumerate() {
        let mut value = serde_json::to_value(part).map_err(storage_error)?;
        if matches!(part, Part::ToolCall { .. } | Part::ToolResult { .. }) {
            let source = source.as_ref().expect("canonical tool entry selected");
            let raw = history::tools::raw(db, source.turn, source.sequence, &source.event, index)
                .await?
                .ok_or_else(|| {
                    Fault::new(ErrorCode::NotFound, "history tool value is unavailable")
                })?;
            let data = value["data"]
                .as_object_mut()
                .expect("tool part serializes an object");
            data.remove("summary");
            if matches!(part, Part::ToolCall { .. }) {
                data.insert("arguments".into(), raw.arguments);
                if let Some(display) = raw.display {
                    data.insert(
                        "display".into(),
                        serde_json::to_value(display).map_err(storage_error)?,
                    );
                }
            } else {
                data.insert("result".into(), raw.result.unwrap_or(Value::Null));
            }
        }
        let encoded = serde_json::to_string(&value).map_err(storage_error)?;
        if encoded.len() > PART_BYTES {
            trimmed = true;
            match part {
                Part::Text(_) | Part::Thinking(_) => {
                    let mut text = value["data"].as_str().unwrap_or_default().to_owned();
                    trim(&mut text, PART_BYTES / 2);
                    value["data"] = Value::String(text);
                }
                Part::ToolResult { .. } => {
                    value["data"]["result"] = serde_json::json!({"truncated":true,"preview":prefix(&serde_json::to_string(&value["data"]["result"]).map_err(storage_error)?, PART_BYTES / 2)});
                    value["data"]["images"] = serde_json::json!([]);
                }
                _ => {
                    value = serde_json::to_value(Part::Resource(serde_json::json!({
                        "type":"truncated_history_part",
                        "preview":prefix(&encoded, PART_BYTES / 2),
                    })))
                    .map_err(storage_error)?;
                }
            }
        }
        let size = serde_json::to_vec(&value).map_err(storage_error)?.len();
        if bytes + size > ENTRY_BYTES || parts.len() == 32 {
            trimmed = true;
            break;
        }
        bytes += size;
        parts.push(value);
    }
    entry.parts.clear();
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
    let mut entry = serde_json::to_value(entry).map_err(storage_error)?;
    entry["parts"] = Value::Array(parts);
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
