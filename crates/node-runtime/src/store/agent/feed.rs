//! One shared ephemeral feed; canonical history stays on the storage worker.
use super::*;
use sailry_link::{CancellationToken, Pending, Subscription};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

#[cfg(test)]
mod tests;

#[derive(Clone, Default)]
pub(in crate::store) struct Feeds(Arc<Mutex<BTreeMap<SessionId, Feed>>>);

const MAX_FINALIZED: usize = 4096;
// Per retained Part, including the serialized string's enclosing quotes.
const PART_OVERHEAD: usize = 34;

struct Feed {
    sequence: u64,
    active: Option<TurnId>,
    drafts: Vec<Draft>,
    // Saturation stops transient publication for this run, never evicts a seal.
    finalized: Option<BTreeSet<String>>,
    bytes: usize,
    updates: broadcast::Sender<Update>,
}

impl Default for Feed {
    fn default() -> Self {
        Self {
            sequence: 0,
            active: None,
            drafts: Vec::new(),
            finalized: Some(BTreeSet::new()),
            bytes: 0,
            updates: broadcast::channel(64).0,
        }
    }
}

impl Feeds {
    pub(super) fn seal(&self, session: SessionId, turn: TurnId) -> Vec<Draft> {
        let mut feeds = self.0.lock().unwrap();
        let Some(feed) = feeds.get_mut(&session) else {
            return Vec::new();
        };
        if feed.active != Some(turn) {
            return Vec::new();
        }
        feed.finalized = None;
        feed.drafts
            .iter()
            .filter(|draft| draft.turn == turn)
            .cloned()
            .collect()
    }

    fn reset(&self, session: SessionId) {
        if let Some(feed) = self.0.lock().unwrap().get_mut(&session) {
            feed.sequence += 1;
            feed.drafts.clear();
            feed.bytes = 0;
            let _ = feed.updates.send(Update::ResetRequired);
        }
    }

    pub(in crate::store) fn sequence(&self, session: SessionId) -> u64 {
        self.0
            .lock()
            .unwrap()
            .get(&session)
            .map_or(0, |feed| feed.sequence)
    }

    fn prune(feeds: &mut BTreeMap<SessionId, Feed>) {
        feeds.retain(|_, feed| feed.active.is_some() || feed.updates.receiver_count() > 0);
    }

    pub(in crate::store) fn publish(&self, node: NodeId, session: SessionId, change: Change) {
        let mut feeds = self.0.lock().unwrap();
        Self::prune(&mut feeds);
        let starts = matches!(&change, Change::Run(run) if run.status == Status::Running);
        if !starts && !feeds.contains_key(&session) {
            return;
        }
        let feed = feeds.entry(session).or_default();
        match &change {
            Change::Run(run) => {
                if run.status == Status::Running {
                    if feed.active != Some(run.turn) {
                        feed.finalized = Some(BTreeSet::new());
                    }
                    feed.active = Some(run.turn);
                }
                if run.status == Status::Stopping && feed.active == Some(run.turn) {
                    // Stop publication precedes controller cancellation on the storage worker.
                    feed.finalized = None;
                }
                if !matches!(
                    run.status,
                    Status::Queued | Status::Running | Status::Stopping
                ) {
                    feed.drafts.retain(|draft| draft.turn != run.turn);
                    if feed.active == Some(run.turn) {
                        feed.active = None;
                        feed.finalized = Some(BTreeSet::new());
                    }
                }
            }
            Change::Entry(entry) => {
                feed.drafts
                    .retain(|draft| draft.turn != entry.turn || draft.id != entry.id);
                if feed.active == Some(entry.turn)
                    && let Some(finalized) = &mut feed.finalized
                {
                    finalized.insert(entry.id.clone());
                    if finalized.len() > MAX_FINALIZED {
                        feed.finalized = None;
                    }
                }
            }
            Change::Delta(_) => unreachable!("partial events use the bounded delta path"),
            Change::Queue(_)
            | Change::Approval(_)
            | Change::Question(_)
            | Change::Child(_)
            | Change::Statistics(_) => {}
            Change::Artifact(_) => {}
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
        &self,
        node: NodeId,
        session: SessionId,
        draft: Draft,
    ) -> Result<(), Fault> {
        let mut feeds = self.0.lock().unwrap();
        let Some(feed) = feeds.get_mut(&session) else {
            return Ok(());
        };
        if feed.active != Some(draft.turn)
            || feed
                .finalized
                .as_ref()
                .is_none_or(|finalized| finalized.contains(&draft.id))
        {
            return Ok(());
        }
        if draft.parts.is_empty() {
            return Ok(());
        }
        let current = feed
            .drafts
            .iter()
            .find(|current| current.id == draft.id && current.turn == draft.turn);
        let mut previous = current.and_then(|current| current.parts.last());
        let mut bytes = 0;
        for part in &draft.parts {
            let merged = matches!(
                (previous, part),
                (Some(Part::Text(_)), Part::Text(_)) | (Some(Part::Thinking(_)), Part::Thinking(_))
            );
            // Adjacent chunks share one retained Part and one pair of JSON quotes.
            // Escaped text payload remains additive when the strings are joined.
            bytes += part_bytes(part) - if merged { PART_OVERHEAD } else { 0 };
            previous = Some(part);
        }
        let Some(total) = feed
            .bytes
            .checked_add(bytes)
            .filter(|total| *total <= 2 * 1024 * 1024)
        else {
            return Err(sailry_protocol::Fault::invalid(
                "streaming output exceeds its buffer limit",
            ));
        };
        let index = if let Some(index) = feed
            .drafts
            .iter()
            .position(|current| current.id == draft.id && current.turn == draft.turn)
        {
            index
        } else {
            let mut retained = draft.clone();
            retained.parts.clear();
            feed.drafts.push(retained);
            feed.drafts.len() - 1
        };
        let current = &mut feed.drafts[index];
        for part in &draft.parts {
            match (current.parts.last_mut(), part) {
                (Some(Part::Text(current)), Part::Text(text))
                | (Some(Part::Thinking(current)), Part::Thinking(text)) => current.push_str(text),
                _ => current.parts.push(part.clone()),
            }
        }
        feed.bytes = total;
        feed.send(node, session, Change::Delta(draft));
        Ok(())
    }

    pub(super) fn subscribe(
        &self,
        node: NodeId,
        history: History,
        statistics: Statistics,
        artifacts: sailry_protocol::artifact::Page,
        closed: CancellationToken,
    ) -> Box<dyn Subscription> {
        let mut feeds = self.0.lock().unwrap();
        Self::prune(&mut feeds);
        let History {
            mut page,
            mut missing,
            ..
        } = history;
        let feed = feeds.entry(page.session).or_default();
        // Leave room for temporary output in the same bounded transport frame.
        let drafts_size = serde_json::to_vec(&feed.drafts)
            .expect("drafts are serializable")
            .len();
        let resources_size = serde_json::to_vec(&(&artifacts, &statistics))
            .expect("conversation resources are serializable")
            .len();
        while !page.entries.is_empty()
            && serde_json::to_vec(&page)
                .expect("conversation is serializable")
                .len()
                + drafts_size
                + resources_size
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
                    artifacts,
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
                + PART_OVERHEAD
                - 2
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
    if let Output::ArtifactWritten { artifact, .. } = output {
        database.feeds.publish(
            database.node,
            artifact.session,
            Change::Artifact(artifact.clone()),
        );
        return;
    }
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
        resolved(database, approval);
        if approval.choice == Some(ApprovalChoice::Session) {
            let approved: Vec<_> = database
                .approvals
                .keys()
                .filter_map(|id| approvals::get(&database.connection, *id).ok())
                .filter(|pending| {
                    pending.session == approval.session
                        && pending.state == ApprovalState::Approved
                        && pending.source == ApprovalSource::SessionRule
                        && pending.choice == Some(ApprovalChoice::Session)
                })
                .collect();
            for approval in approved {
                resolved(database, &approval);
            }
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

fn resolved(database: &mut Database, approval: &Approval) {
    database.feeds.publish(
        database.node,
        approval.session,
        Change::Approval(approval.clone()),
    );
    if let Some((_, reply)) = database.approvals.remove(&approval.id) {
        let decision = match approval.state {
            ApprovalState::Approved => Decision::Approve,
            ApprovalState::Denied => approval
                .reason
                .clone()
                .map_or(Decision::Deny, Decision::DenyWithReason),
            _ => unreachable!("only resolved approvals are command results"),
        };
        let _ = reply.send(Ok(decision));
    }
}
