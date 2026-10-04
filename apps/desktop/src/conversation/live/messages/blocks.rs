use sailry_client::conversation::{View, tools::Call};
use sailry_protocol::{
    SessionId, TurnId,
    conversation::{Entry, Part},
};
use std::{borrow::Cow, collections::BTreeMap};

pub(super) enum Block<'a> {
    Text(String, Cow<'a, str>),
    Search(&'a Entry),
    Image(&'a sailry_protocol::conversation::Image),
    Thinking(String, Cow<'a, str>),
    Compaction(String, &'a str),
    Tools(Vec<&'a Call>),
    Question(&'a Call),
    Children(Vec<SessionId>),
    Resource,
    Retry(u64, u64),
}

/// Visual grouping only; call identities and results come from the shared Client.
pub(super) fn collect(turn: TurnId, history: &View) -> Vec<Block<'_>> {
    let Some(snapshot) = &history.snapshot else {
        return Vec::new();
    };
    let calls: BTreeMap<_, _> = history
        .calls
        .iter()
        .filter(|call| call.turn == turn)
        .map(|call| (call.source.key(), call))
        .collect();
    let latest_progress = history
        .calls
        .iter()
        .rev()
        .find(|call| call.turn == turn && call.progress.is_some());
    let mut progress_shown = false;
    let parts = snapshot
        .page
        .entries
        .iter()
        .filter(|entry| entry.turn == turn && entry.author != "user")
        .flat_map(|entry| {
            let mut offset = 0;
            let last = entry
                .parts
                .iter()
                .rposition(|part| matches!(part, Part::Text(_)));
            let mut search = entry.search_suggestions.as_ref().map(|_| entry);
            entry
                .parts
                .iter()
                .enumerate()
                .filter_map(move |(index, part)| {
                    if matches!(part, Part::Text(_)) && entry.search_suggestions.is_some() {
                        return search.take().map(|entry| {
                            (
                                format!("{}-{index}", entry.id),
                                part,
                                None,
                                Some(entry),
                                false,
                            )
                        });
                    }
                    let text = if let Part::Text(text) = part {
                        let value = super::citations::text(
                            text,
                            offset,
                            last == Some(index),
                            &entry.citations,
                        );
                        offset += text.chars().count();
                        Some(value)
                    } else {
                        None
                    };
                    Some((
                        format!("{}-{index}", entry.id),
                        part,
                        text,
                        None,
                        continues(&entry.parts, index),
                    ))
                })
        })
        .chain(
            snapshot
                .drafts
                .iter()
                .filter(|draft| draft.turn == turn)
                .flat_map(|draft| {
                    draft.parts.iter().enumerate().map(|(index, part)| {
                        (
                            format!("draft-{}-{index}", draft.id),
                            part,
                            None,
                            None,
                            continues(&draft.parts, index),
                        )
                    })
                }),
        );
    let mut blocks = Vec::new();
    for (key, part, text, search, continuation) in parts {
        if let Some(entry) = search {
            blocks.push(Block::Search(entry));
            continue;
        }
        match part {
            Part::Text(original) => {
                let text = text.unwrap_or(Cow::Borrowed(original));
                if continuation && let Some(Block::Text(_, previous)) = blocks.last_mut() {
                    previous.to_mut().push_str(&text);
                } else {
                    blocks.push(Block::Text(key, text));
                }
            }
            Part::Image(image) => blocks.push(Block::Image(image)),
            Part::Thinking(text) => {
                if continuation && let Some(Block::Thinking(_, previous)) = blocks.last_mut() {
                    previous.to_mut().push_str(text);
                } else {
                    blocks.push(Block::Thinking(key, Cow::Borrowed(text)));
                }
            }
            Part::Compaction(text) => blocks.push(Block::Compaction(key, text)),
            Part::Resource(value)
                if value["type"] == "provider_context" || value["type"] == "tool_started" => {}
            Part::Resource(value) if value["type"] == "model_retry" => {
                if let (Some(attempt), Some(limit)) =
                    (value["attempt"].as_u64(), value["limit"].as_u64())
                {
                    if let Some(Block::Retry(previous, previous_limit)) = blocks.last_mut()
                        && *previous_limit == limit
                        && attempt > *previous
                    {
                        *previous = attempt;
                    } else {
                        blocks.push(Block::Retry(attempt, limit));
                    }
                }
            }
            Part::Resource(_) | Part::Attachment(_) | Part::Reference(_) => {
                blocks.push(Block::Resource)
            }
            Part::ToolCall { .. } | Part::ToolResult { .. } => {
                if let Some(call) = calls.get(&key) {
                    if call.progress.is_some() {
                        if progress_shown {
                            continue;
                        }
                        progress_shown = true;
                        // Keep one stable position while rendering the latest successful
                        // snapshot. The canonical tool history remains unchanged.
                        blocks.push(Block::Tools(vec![latest_progress.unwrap()]));
                        continue;
                    }
                    // Represent admitted delegations through their child rows; keep
                    // calls that failed before a child was created as tool errors.
                    if let Some(child) = snapshot.page.children.iter().find(|child| {
                        child.origin.turn == turn
                            && child.origin.entry == call.source.entry
                            && child.origin.index == call.source.index
                    }) {
                        match blocks.last_mut() {
                            Some(Block::Children(group)) => group.push(child.run.session),
                            _ => blocks.push(Block::Children(vec![child.run.session])),
                        }
                        continue;
                    }
                    if call.name == "compact_context"
                        && let Some(summary) = call
                            .result(&snapshot.page)
                            .and_then(|result| result["summary_id"].as_str())
                        && snapshot.page.entries.iter().any(|entry| {
                            entry.id == summary
                                && entry
                                    .parts
                                    .iter()
                                    .any(|part| matches!(part, Part::Compaction(_)))
                        })
                    {
                        continue;
                    }
                    if call.question_spec(&snapshot.page).is_some() {
                        blocks.push(Block::Question(call));
                        continue;
                    }
                    match blocks.last_mut() {
                        Some(Block::Tools(group))
                            if groupable(call) && group.iter().all(|call| groupable(call)) =>
                        {
                            group.push(*call)
                        }
                        _ => blocks.push(Block::Tools(vec![*call])),
                    }
                }
            }
        }
    }
    // Keep whitespace while joining streamed chunks, but do not render a
    // standalone blank response as a paragraph between tool rows.
    blocks.retain(|block| !matches!(block, Block::Text(_, text) if text.trim().is_empty()));
    // Empty streamed messages do not divide an otherwise consecutive tool sequence.
    join_tools(blocks)
}

/// Hide reasoning only in the visual projection, then regroup adjacent tools.
pub(super) fn compact(mut blocks: Vec<Block<'_>>) -> Vec<Block<'_>> {
    blocks.retain(|block| !matches!(block, Block::Thinking(..)));
    join_tools(blocks)
}

fn join_tools(blocks: Vec<Block<'_>>) -> Vec<Block<'_>> {
    let mut joined = Vec::with_capacity(blocks.len());
    for block in blocks {
        match (joined.last_mut(), block) {
            (Some(Block::Tools(previous)), Block::Tools(calls))
                if previous.iter().chain(&calls).all(|call| groupable(call)) =>
            {
                previous.extend(calls)
            }
            (_, block) => joined.push(block),
        }
    }
    joined
}

fn groupable(call: &Call) -> bool {
    call.grouping == sailry_protocol::tool::Grouping::Sequence
        && call.presentation != sailry_protocol::tool::Presentation::Progress
        && call.progress.is_none()
}

fn continues(parts: &[Part], index: usize) -> bool {
    index.checked_sub(1).is_some_and(|previous| {
        matches!(
            (&parts[previous], &parts[index]),
            (Part::Text(_), Part::Text(_)) | (Part::Thinking(_), Part::Thinking(_))
        )
    })
}

#[cfg(test)]
mod tests;
