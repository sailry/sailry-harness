//! Hydrate complete turns before exposing another logical history page.
use super::projection::{invalid, validate_page};
use crate::Client;
use sailry_protocol::{
    conversation::{History, Page, TurnHistory},
    *,
};
use std::collections::{BTreeMap, BTreeSet};

impl Client {
    pub async fn read_conversation(
        &self,
        session: SessionId,
        before: Option<TurnId>,
        limit: u16,
    ) -> Result<History, Fault> {
        let Output::Conversation(history) = self
            .execute(self.prepare(Command::ReadConversation {
                session,
                before,
                limit,
            }))
            .await?
        else {
            return Err(invalid("conversation page response expected"));
        };
        if history.page.session != session {
            return Err(invalid("history belongs to another conversation"));
        }
        self.complete_history(history).await
    }

    pub(super) async fn complete_history(&self, mut history: History) -> Result<History, Fault> {
        validate_page(&history.page)?;
        let mut missing = BTreeSet::new();
        for turn in &history.missing {
            if !missing.insert(*turn) || !history.page.runs.iter().any(|run| run.turn == *turn) {
                return Err(invalid(
                    "incomplete history references an unknown or repeated turn",
                ));
            }
        }
        for turn in missing {
            let mut before = history
                .page
                .entries
                .iter()
                .find(|entry| entry.turn == turn)
                .map(|entry| entry.sequence);
            loop {
                let Output::TurnHistory(chunk) = self
                    .execute(self.prepare(Command::ReadTurn {
                        session: history.page.session,
                        turn,
                        expected_revision: history.page.revision,
                        before,
                        limit: 100,
                    }))
                    .await?
                else {
                    return Err(invalid("turn continuation response expected"));
                };
                if chunk.run.session != history.page.session || chunk.run.turn != turn {
                    return Err(invalid("turn continuation belongs to another read"));
                }
                if chunk.entries.is_empty()
                    || chunk.entries.iter().any(|entry| {
                        entry.turn != turn || before.is_some_and(|before| entry.sequence >= before)
                    })
                    || chunk.next_before.is_some_and(|next| {
                        chunk
                            .entries
                            .first()
                            .is_none_or(|entry| entry.sequence != next)
                    })
                {
                    return Err(invalid(
                        "turn continuation did not advance within its boundary",
                    ));
                }
                history.sequence = history.sequence.max(chunk.sequence);
                let next = chunk.next_before;
                merge_turn(&mut history.page, chunk)?;
                let Some(next) = next else {
                    break;
                };
                before = Some(next);
            }
        }
        history.missing.clear();
        Ok(history)
    }
}

fn merge_turn(page: &mut Page, chunk: TurnHistory) -> Result<(), Fault> {
    let incoming = Page {
        session: chunk.run.session,
        revision: chunk.revision,
        runs: vec![chunk.run],
        entries: chunk.entries,
        approvals: chunk.approvals,
        questions: chunk.questions,
        children: chunk.children,
        queue: Default::default(),
        next_before: None,
    };
    merge(page, incoming, false)
}

/// Older transport results must never roll back state already received on the stream.
pub(super) fn merge(current: &mut Page, incoming: Page, keep_current: bool) -> Result<(), Fault> {
    validate_page(&incoming)?;
    if current.session != incoming.session {
        return Err(invalid("history merge belongs to another conversation"));
    }
    if current.revision != incoming.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "conversation history changed during pagination",
        ));
    }
    let mut merged = current.clone();
    let mut entries: BTreeMap<_, _> = merged
        .entries
        .into_iter()
        .map(|entry| (entry.sequence, entry))
        .collect();
    let mut identities: BTreeMap<_, _> = entries
        .values()
        .map(|entry| (entry.id.clone(), entry.sequence))
        .collect();
    for entry in incoming.entries {
        if let Some(previous) = entries.get(&entry.sequence) {
            if previous != &entry {
                return Err(invalid("canonical history changed during pagination"));
            }
        } else {
            if identities
                .insert(entry.id.clone(), entry.sequence)
                .is_some()
            {
                return Err(invalid("canonical event identity moved during pagination"));
            }
            entries.insert(entry.sequence, entry);
        }
    }
    merged.entries = entries.into_values().collect();
    for run in incoming.runs {
        if let Some(previous) = merged
            .runs
            .iter_mut()
            .find(|previous| previous.turn == run.turn)
        {
            if previous.sequence != run.sequence
                || previous.kind != run.kind
                || previous.revision != run.revision
                || previous.worktree != run.worktree
                || previous.origin != run.origin
            {
                return Err(invalid("sent turn identity changed during pagination"));
            }
            if !keep_current {
                *previous = run;
            }
        } else {
            merged.runs.push(run);
        }
    }
    merged.runs.sort_by_key(|run| run.sequence);
    for child in incoming.children {
        super::projection::children::merge(&mut merged, child, keep_current)?;
    }
    for approval in incoming.approvals {
        if let Some(previous) = merged
            .approvals
            .iter_mut()
            .find(|previous| previous.id == approval.id)
        {
            if previous.turn != approval.turn
                || previous.entry != approval.entry
                || previous.index != approval.index
                || previous.source != approval.source
                || previous.state != conversation::ApprovalState::Pending
                    && approval.state != conversation::ApprovalState::Pending
                    && previous.state != approval.state
            {
                return Err(invalid(
                    "approval identity or terminal decision changed during pagination",
                ));
            }
            if !keep_current {
                *previous = approval;
            }
        } else {
            merged.approvals.push(approval);
        }
    }
    for question in incoming.questions {
        if let Some(previous) = merged
            .questions
            .iter_mut()
            .find(|previous| previous.id == question.id)
        {
            if previous.turn != question.turn
                || previous.entry != question.entry
                || previous.index != question.index
                || previous.state != conversation::question::State::Pending
                    && question.state != conversation::question::State::Pending
                    && previous.state != question.state
            {
                return Err(invalid(
                    "question identity or terminal answer changed during pagination",
                ));
            }
            if !keep_current {
                *previous = question;
            }
        } else {
            merged.questions.push(question);
        }
    }
    merged.approvals.sort_by_key(|approval| {
        (
            merged
                .entries
                .iter()
                .find(|entry| entry.id == approval.entry)
                .map_or(u64::MAX, |entry| entry.sequence),
            approval.index,
        )
    });
    merged.questions.sort_by_key(|question| {
        (
            merged
                .entries
                .iter()
                .find(|entry| entry.id == question.entry)
                .map_or(u64::MAX, |entry| entry.sequence),
            question.index,
        )
    });
    validate_page(&merged)?;
    *current = merged;
    Ok(())
}
