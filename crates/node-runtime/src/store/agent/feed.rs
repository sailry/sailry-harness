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

pub(in crate::store) struct Reserved {
    sequence: u64,
    drafts: Vec<Draft>,
    updates: broadcast::Receiver<Update>,
}

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
            Change::Artifact(_) | Change::Background(_) => {}
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

    #[cfg(test)]
    pub(super) fn subscribe(
        &self,
        node: NodeId,
        history: History,
        statistics: Statistics,
        artifacts: sailry_protocol::artifact::Page,
        closed: CancellationToken,
    ) -> Result<Box<dyn Subscription>, Fault> {
        self.reserve(history.page.session)
            .subscribe(node, history, statistics, artifacts, closed)
    }

    pub(in crate::store) fn reserve(&self, session: SessionId) -> Reserved {
        let mut feeds = self.0.lock().unwrap();
        Self::prune(&mut feeds);
        let feed = feeds.entry(session).or_default();
        Reserved {
            sequence: feed.sequence,
            drafts: feed.drafts.clone(),
            updates: feed.updates.subscribe(),
        }
    }
}

impl Reserved {
    pub(in crate::store) fn subscribe(
        self,
        node: NodeId,
        history: History,
        statistics: Statistics,
        artifacts: sailry_protocol::artifact::Page,
        closed: CancellationToken,
    ) -> Result<Box<dyn Subscription>, Fault> {
        let History {
            mut page,
            mut missing,
            ..
        } = history;
        // Leave room for temporary output in the same bounded transport frame.
        let drafts_size = serde_json::to_vec(&self.drafts)
            .expect("drafts are serializable")
            .len();
        let resources_size = serde_json::to_vec(&(&artifacts, &statistics))
            .expect("conversation resources are serializable")
            .len();
        let trimmed = super::history::pages::fit(&mut page, drafts_size + resources_size, true)?;
        missing.retain(|turn| page.runs.iter().any(|run| run.turn == *turn));
        for turn in trimmed {
            if !missing.contains(&turn) {
                missing.push(turn);
            }
        }
        Ok(Box::new(Updates {
            initial: Some(Update::ConversationSnapshot(Box::new(
                sailry_protocol::conversation::Snapshot {
                    node,
                    sequence: self.sequence,
                    page: Arc::new(page),
                    missing,
                    drafts: self.drafts,
                    statistics,
                    artifacts,
                },
            ))),
            updates: self.updates,
            closed,
        }))
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

pub(in crate::store) async fn package(database: &mut Database, name: &str) {
    if let Ok(sessions) = super::continuation::sessions(&database.connection, name).await {
        for session in sessions {
            database.feeds.reset(session);
        }
    }
}

pub(in crate::store) async fn command(database: &mut Database, output: &Output) {
    if let Output::Session(session) = output {
        statistics(database, session.id).await;
    }
    if matches!(output, Output::Provider(_)) {
        let sessions = database
            .feeds
            .0
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for session in sessions {
            statistics(database, session).await;
        }
    }

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
            Box::pin(command(database, output)).await;
        }
        return;
    }
    if let Output::TurnReplaced { turn, history } = output {
        database.feeds.reset(history.session);
        Box::pin(command(database, &Output::QueuedTurn(turn.clone()))).await;
        return;
    }
    if let Output::PlanAccepted(accepted) = output {
        Box::pin(command(
            database,
            &Output::QueuedTurn(accepted.turn.clone()),
        ))
        .await;
        Box::pin(command(
            database,
            &Output::Question(accepted.question.clone()),
        ))
        .await;
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
        if let Some((_, reply)) = database.questions.lock().unwrap().remove(&question.id) {
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
        resolved(database, approval).await;
        if approval.choice == Some(ApprovalChoice::Session) {
            let mut approved = Vec::new();
            let ids: Vec<_> = database.approvals.lock().unwrap().keys().copied().collect();
            for id in ids {
                if let Ok(pending) = approvals::get(&database.connection, id).await
                    && pending.session == approval.session
                    && pending.state == ApprovalState::Approved
                    && pending.source == ApprovalSource::SessionRule
                    && pending.choice == Some(ApprovalChoice::Session)
                {
                    approved.push(pending);
                }
            }
            for approval in approved {
                resolved(database, &approval).await;
            }
        }
        return;
    }
    if let Output::Queue(update) = output {
        for current in &update.runs {
            run(database, current.clone()).await;
        }
        if let Ok(tasks) = super::background::list(&database.connection, update.session).await {
            super::background::publish(database, tasks).await;
        }
        database.feeds.publish(
            database.node,
            update.session,
            Change::Queue(update.queue.clone()),
        );
        return;
    }
    let run = match output {
        Output::QueuedTurn(turn) => runs::get(&database.connection, turn.id).await.ok(),
        Output::Run(run) => Some(run.clone()),
        _ => None,
    };
    if let Some(run) = run {
        if let Ok(Some(child)) = super::children::get(&database.connection, &run).await {
            database
                .feeds
                .publish(database.node, child.origin.session, Change::Child(child));
        }
        if let Ok(queue) = super::queue::read(&database.connection, run.session).await {
            database
                .feeds
                .publish(database.node, run.session, Change::Queue(queue));
        }
        let session = run.session;
        self::run(database, run).await;
        if let Ok(tasks) = super::background::list(&database.connection, session).await {
            super::background::publish(database, tasks).await;
        }
    }
}

/// Conversation-only capture hydration must not change a committed business result.
pub(in crate::store) async fn run(database: &Database, current: Run) {
    let session = current.session;
    match runs::hydrate(&database.connection, current).await {
        Ok(current) => database
            .feeds
            .publish(database.node, session, Change::Run(current)),
        Err(_) => database.feeds.reset(session),
    }
    statistics(database, session).await;
}

async fn statistics(database: &Database, session: SessionId) {
    match super::statistics::read(&database.connection, session).await {
        Ok(statistics) => database.feeds.publish(
            database.node,
            session,
            Change::Statistics(Box::new(statistics)),
        ),
        Err(_) => database.feeds.reset(session),
    }
}

async fn resolved(database: &mut Database, approval: &Approval) {
    database.feeds.publish(
        database.node,
        approval.session,
        Change::Approval(approval.clone()),
    );
    if let Some((_, _, _, Some(reply))) = database.approvals.lock().unwrap().remove(&approval.id) {
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
