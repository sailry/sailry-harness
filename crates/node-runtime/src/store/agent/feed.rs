//! Ephemeral output and bounded subscriptions, owned by the storage worker.
use super::*;
use sailry_link::{CancellationToken, Pending, Subscription};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(in crate::store) struct Feeds(BTreeMap<SessionId, Feed>);

struct Feed {
    sequence: u64,
    active: Option<TurnId>,
    drafts: Vec<Draft>,
    bytes: usize,
    updates: broadcast::Sender<Update>,
}

impl Default for Feed {
    fn default() -> Self {
        Self {
            sequence: 0,
            active: None,
            drafts: Vec::new(),
            bytes: 0,
            updates: broadcast::channel(64).0,
        }
    }
}

impl Feeds {
    fn reset(&mut self, session: SessionId) {
        if let Some(feed) = self.0.get_mut(&session) {
            feed.sequence += 1;
            feed.drafts.clear();
            feed.bytes = 0;
            let _ = feed.updates.send(Update::ResetRequired);
        }
    }

    pub(in crate::store) fn sequence(&self, session: SessionId) -> u64 {
        self.0.get(&session).map_or(0, |feed| feed.sequence)
    }

    fn prune(&mut self) {
        self.0
            .retain(|_, feed| feed.active.is_some() || feed.updates.receiver_count() > 0);
    }

    pub(in crate::store) fn publish(&mut self, node: NodeId, session: SessionId, change: Change) {
        self.prune();
        let starts = matches!(&change, Change::Run(run) if run.status == Status::Running);
        if !starts && !self.0.contains_key(&session) {
            return;
        }
        let feed = self.0.entry(session).or_default();
        match &change {
            Change::Run(run) => {
                if run.status == Status::Running {
                    feed.active = Some(run.turn);
                }
                if !matches!(
                    run.status,
                    Status::Queued | Status::Running | Status::Stopping
                ) {
                    feed.drafts.retain(|draft| draft.turn != run.turn);
                    if feed.active == Some(run.turn) {
                        feed.active = None;
                    }
                }
            }
            Change::Entry(entry) => feed
                .drafts
                .retain(|draft| draft.turn != entry.turn || draft.id != entry.id),
            Change::Delta(_) => unreachable!("partial events use the bounded delta path"),
            Change::Queue(_)
            | Change::Approval(_)
            | Change::Question(_)
            | Change::Child(_)
            | Change::Statistics(_) => {}
        }
        feed.bytes = feed
            .drafts
            .iter()
            .flat_map(|draft| &draft.parts)
            .map(part_bytes)
            .sum();
        feed.send(node, session, change);
    }

    pub(super) fn delta(
        &mut self,
        node: NodeId,
        session: SessionId,
        draft: Draft,
    ) -> Result<(), Fault> {
        let Some(feed) = self.0.get_mut(&session) else {
            return Ok(());
        };
        if feed.active != Some(draft.turn) {
            return Ok(());
        }
        if draft.parts.is_empty() {
            return Ok(());
        }
        let bytes: usize = draft.parts.iter().map(part_bytes).sum();
        if feed.bytes + bytes > 2 * 1024 * 1024 {
            return Err(invalid("streaming output exceeds its buffer limit"));
        }
        if let Some(current) = feed
            .drafts
            .iter_mut()
            .find(|current| current.id == draft.id && current.turn == draft.turn)
        {
            for part in &draft.parts {
                match (current.parts.last_mut(), part) {
                    (Some(Part::Text(current)), Part::Text(text))
                    | (Some(Part::Thinking(current)), Part::Thinking(text)) => {
                        current.push_str(text)
                    }
                    _ => current.parts.push(part.clone()),
                }
            }
        } else {
            feed.drafts.push(draft.clone());
        }
        feed.bytes += bytes;
        feed.send(node, session, Change::Delta(draft));
        Ok(())
    }

    pub(super) fn subscribe(
        &mut self,
        node: NodeId,
        history: History,
        statistics: Statistics,
        closed: CancellationToken,
    ) -> Box<dyn Subscription> {
        self.prune();
        let History {
            mut page,
            mut missing,
            ..
        } = history;
        let feed = self.0.entry(page.session).or_default();
        // Leave room for temporary output in the same bounded transport frame.
        let drafts_size = serde_json::to_vec(&feed.drafts)
            .expect("drafts are serializable")
            .len();
        while !page.entries.is_empty()
            && serde_json::to_vec(&page)
                .expect("conversation is serializable")
                .len()
                + drafts_size
                > MAX_FRAME_BYTES - 64 * 1024
        {
            let turn = page.entries.remove(0).turn;
            if !missing.contains(&turn) {
                missing.push(turn);
            }
            super::history::pages::retain_metadata(&mut page);
        }
        Box::new(Updates {
            initial: Some(Update::ConversationSnapshot(
                sailry_protocol::conversation::Snapshot {
                    node,
                    sequence: feed.sequence,
                    page: Arc::new(page),
                    missing,
                    drafts: feed.drafts.clone(),
                    statistics,
                },
            )),
            updates: feed.updates.subscribe(),
            closed,
        })
    }
}

fn part_bytes(part: &Part) -> usize {
    match part {
        Part::Text(text) | Part::Thinking(text) => {
            serde_json::to_vec(text)
                .expect("text is serializable")
                .len()
                + 32
        }
        _ => 0,
    }
}

impl Feed {
    fn send(&mut self, node: NodeId, session: SessionId, change: Change) {
        self.sequence += 1;
        let _ = self.updates.send(Update::ConversationFrame(Frame {
            node,
            session,
            sequence: self.sequence,
            change,
        }));
    }
}

struct Updates {
    initial: Option<Update>,
    updates: broadcast::Receiver<Update>,
    closed: CancellationToken,
}
impl Subscription for Updates {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if self.closed.is_cancelled() {
                return Err(super::super::unavailable());
            }
            if let Some(initial) = self.initial.take() {
                return Ok(initial);
            }
            tokio::select! {
                biased;
                _ = self.closed.cancelled() => Err(super::super::unavailable()),
                update = self.updates.recv() => match update {
                    Ok(update) => Ok(update),
                    Err(broadcast::error::RecvError::Lagged(_)) => Ok(Update::ResetRequired),
                    Err(broadcast::error::RecvError::Closed) => Err(super::super::unavailable()),
                }
            }
        })
    }
}

pub(in crate::store) fn package(database: &mut Database, name: &str) {
    if let Ok(sessions) = super::continuation::sessions(&database.connection, name) {
        for session in sessions {
            database.feeds.reset(session);
        }
    }
}

pub(in crate::store) fn command(database: &mut Database, output: &Output) {
    if let Output::PluginTransaction(outputs) = output {
        for output in outputs {
            command(database, output);
        }
        return;
    }
    if let Output::TurnReplaced { turn, history } = output {
        database.feeds.reset(history.session);
        command(database, &Output::QueuedTurn(turn.clone()));
        return;
    }
    if let Output::PlanAccepted(accepted) = output {
        command(database, &Output::QueuedTurn(accepted.turn.clone()));
        command(database, &Output::Question(accepted.question.clone()));
        return;
    }
    if let Output::Rewound(rewind) = output {
        database.feeds.reset(rewind.session);
        return;
    }
    if let Output::Question(question) = output {
        database.feeds.publish(
            database.node,
            question.session,
            Change::Question(question.clone()),
        );
        if let Some((_, reply)) = database.questions.remove(&question.id) {
            let response = match &question.state {
                question::State::Answered(answer) => question::Response::Answer(answer.clone()),
                question::State::Cancelled => question::Response::Cancel,
                question::State::Declined => question::Response::Decline,
                _ => unreachable!("only resolved questions are command results"),
            };
            let _ = reply.send(Ok(response));
        }
        return;
    }
    if let Output::Approval(approval) = output {
        database.feeds.publish(
            database.node,
            approval.session,
            Change::Approval(approval.clone()),
        );
        if let Some((_, reply)) = database.approvals.remove(&approval.id) {
            let decision = match approval.state {
                ApprovalState::Approved => Decision::Approve,
                ApprovalState::Denied => Decision::Deny,
                _ => unreachable!("only resolved approvals are command results"),
            };
            let _ = reply.send(Ok(decision));
        }
        return;
    }
    if let Output::Queue(update) = output {
        for run in &update.runs {
            database
                .feeds
                .publish(database.node, update.session, Change::Run(run.clone()));
        }
        database.feeds.publish(
            database.node,
            update.session,
            Change::Queue(update.queue.clone()),
        );
        return;
    }
    let run = match output {
        Output::QueuedTurn(turn) => runs::get(&database.connection, turn.id).ok(),
        Output::Run(run) => Some(run.clone()),
        _ => None,
    };
    if let Some(run) = run {
        if let Ok(Some(child)) = super::children::get(&database.connection, &run) {
            database
                .feeds
                .publish(database.node, child.origin.session, Change::Child(child));
        }
        if let Ok(queue) = super::queue::read(&database.connection, run.session) {
            database
                .feeds
                .publish(database.node, run.session, Change::Queue(queue));
        }
        database
            .feeds
            .publish(database.node, run.session, Change::Run(run));
    }
}
