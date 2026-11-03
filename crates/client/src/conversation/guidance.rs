//! Guidance delivery is derived from canonical input and the Node's queue.
use sailry_protocol::{TurnId, conversation::Page};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Pending,
    Delivered,
    NextTurn,
}

/// A joined run alone is a claim, not evidence that the model received input.
pub fn project(page: &Page) -> BTreeMap<TurnId, State> {
    let canonical: BTreeSet<_> = page
        .entries
        .iter()
        .filter(|entry| entry.author == "user")
        .map(|entry| entry.turn)
        .collect();
    let queued: BTreeMap<_, _> = page
        .queue
        .items
        .iter()
        .map(|item| (item.turn, item))
        .collect();
    let mut states = BTreeMap::new();
    for run in &page.runs {
        let pending = queued.get(&run.turn);
        let Some(recipient) = run
            .guidance
            .or_else(|| pending.and_then(|item| item.guidance))
        else {
            continue;
        };
        let state = if canonical.contains(&run.turn) && run.joined == Some(recipient) {
            State::Delivered
        } else if let Some(item) = pending {
            if item.guidance.is_some() {
                State::Pending
            } else {
                State::NextTurn
            }
        } else if run.joined == Some(recipient)
            || run.status == sailry_protocol::conversation::Status::Queued
        {
            State::Pending
        } else {
            State::NextTurn
        };
        states.insert(run.turn, state);
    }
    for item in &page.queue.items {
        if item.guidance.is_some() {
            states.entry(item.turn).or_insert(State::Pending);
        }
    }
    states
}

#[cfg(test)]
mod tests;
