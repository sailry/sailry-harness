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
    let rows = sailry_client::conversation::rows::collect(snapshot, &history.calls, turn);
    let mut blocks = Vec::new();
    let mut searches = std::collections::BTreeSet::new();
    for row in rows {
        let key = if row.draft {
            format!("draft-{}-{}", row.source.entry, row.source.index)
        } else {
            row.source.key()
        };
        // Desktop renders result images inside the associated tool card.
        if let Some(image) = row.image {
            if !history
                .calls
                .iter()
                .any(|call| call.response.as_ref() == Some(&row.source))
            {
                blocks.push(Block::Images(vec![image]));
            }
            continue;
        }
        let part = row.part;
        let continuation = row.continuation;
        if matches!(part, Part::Text(_))
            && let Some(entry) = row.entry.filter(|entry| entry.search_suggestions.is_some())
        {
            if searches.insert(entry.id.as_str()) {
                blocks.push(Block::Search(entry));
            }
            continue;
        }
        match part {
            Part::Text(original) => {
                let text = row.text.unwrap_or_else(|| original.clone());
                let text = match row.entry {
                    Some(entry) => Cow::Owned(
                        super::citations::text(
                            &text,
                            row.text_offset,
                            row.text_last,
                            &entry.citations,
                        )
                        .into_owned(),
                    ),
                    None => Cow::Owned(text),
                };
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
                blocks.push(Block::Thinking(
                    key,
                    Cow::Owned(row.text.unwrap_or_else(|| text.clone())),
                ));
            }
            Part::Compaction(text) => blocks.push(Block::Compaction(key, text)),
            Part::Resource(value) if value["type"] == "compaction_failure" => {
                if let Some(failure) =
                    sailry_protocol::conversation::compaction::Failure::from_resource(value)
                {
                    blocks.push(Block::CompactionFailure(key, failure));
                }
            }
            Part::Resource(value) if value["type"] == "model_retry" => {
                if let Some(retry) =
                    sailry_protocol::conversation::recovery::Retry::from_resource(value)
                {
                    blocks.push(Block::Retry(key, retry));
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
                    let children: Vec<_> = call
                        .children(&snapshot.page)
                        .into_iter()
                        .map(|reference| &snapshot.page.children[reference.index])
                        .collect();
                    if !children.is_empty() {
                        match blocks.last_mut() {
                            Some(Block::Children(group)) => group.extend(children),
                            _ => blocks.push(Block::Children(children)),
                        }
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
    // Empty streamed messages do not divide an otherwise consecutive activity sequence.
    join(blocks)
}

/// Hide reasoning only in the visual projection, then regroup adjacent activity.
pub(super) fn compact(mut blocks: Vec<Block<'_>>) -> Vec<Block<'_>> {
    blocks.retain(|block| !matches!(block, Block::Thinking(..)));
    join(blocks)
}

fn join(blocks: Vec<Block<'_>>) -> Vec<Block<'_>> {
    let mut joined = Vec::with_capacity(blocks.len());
    for block in blocks {
        match (joined.last_mut(), block) {
            (Some(Block::Tools(previous)), Block::Tools(calls))
                if previous.iter().chain(&calls).all(|call| groupable(call)) =>
            {
                previous.extend(calls)
            }
            (Some(Block::Children(previous)), Block::Children(children)) => {
                previous.extend(children)
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

#[cfg(test)]
mod tests;
