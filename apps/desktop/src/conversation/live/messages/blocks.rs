use sailry_client::conversation::{View, tools::Call};
use sailry_protocol::{
    TurnId,
    conversation::{Child, Entry, Part},
};
use std::{borrow::Cow, collections::BTreeMap};

pub(super) enum Block<'a> {
    Text(String, Cow<'a, str>),
    Search(&'a Entry),
    Images(Vec<&'a sailry_protocol::conversation::Image>),
    Thinking(String, Cow<'a, str>),
    Compaction(String, &'a str),
    CompactionFailure(String, sailry_protocol::conversation::compaction::Failure),
    Tools(Vec<&'a Call>),
    Question(&'a Call),
    Children(Vec<&'a Child>),
    Resource,
    Retry(String, sailry_protocol::conversation::recovery::Retry),
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
            Part::Image(image) => {
                if continuation && let Some(Block::Images(previous)) = blocks.last_mut() {
                    previous.push(image);
                } else {
                    blocks.push(Block::Images(vec![image]));
                }
            }
            Part::Thinking(text) => {
                if continuation && let Some(Block::Thinking(_, previous)) = blocks.last_mut() {
                    previous.to_mut().push_str(text);
                } else {
                    blocks.push(Block::Thinking(key, Cow::Borrowed(text)));
                }
            }
            Part::Compaction(text) => blocks.push(Block::Compaction(key, text)),
            Part::Resource(value) if value["type"] == "compaction_failure" => {
                if let Some(failure) =
                    sailry_protocol::conversation::compaction::Failure::from_resource(value)
                {
                    blocks.push(Block::CompactionFailure(key, failure));
                }
            }
            Part::Resource(value)
                if value["type"] == "provider_context"
                    || value["type"] == "tool_started"
                    || value["type"] == "tool_failed"
                    || (value["type"] == "operation_warning"
                        && value["code"] == "context_summary_failed") => {}
            Part::Resource(value) if value["type"] == "model_retry" => {
                if let Some(retry) =
                    sailry_protocol::conversation::recovery::Retry::from_resource(value)
                {
                    if let Some(Block::Retry(_, previous)) = blocks.last_mut()
                        && !previous.fallback
                        && !retry.fallback
                        && previous.limit == retry.limit
                        && previous.transport == retry.transport
                        && retry.attempt > previous.attempt
                    {
                        *previous = retry;
                    } else {
                        blocks.push(Block::Retry(key, retry));
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
                    let children = super::super::subagents::latest(
                        snapshot.page.children.iter().filter(|child| {
                            child.origin.turn == turn
                                && child.origin.entry == call.source.entry
                                && child.origin.index == call.source.index
                        }),
                    );
                    if !children.is_empty() {
                        match blocks.last_mut() {
                            Some(Block::Children(group)) => group.extend(children),
                            _ => blocks.push(Block::Children(children)),
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
                    if call.hidden(&snapshot.page) {
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
    !call.terminal_interaction
        && call.grouping == sailry_protocol::tool::Grouping::Sequence
        && call.presentation != sailry_protocol::tool::Presentation::Progress
        && call.progress.is_none()
}

fn continues(parts: &[Part], index: usize) -> bool {
    index.checked_sub(1).is_some_and(|previous| {
        matches!(
            (&parts[previous], &parts[index]),
            (Part::Text(_), Part::Text(_))
                | (Part::Thinking(_), Part::Thinking(_))
                | (Part::Image(_), Part::Image(_))
        )
    })
}

#[cfg(test)]
mod tests;
