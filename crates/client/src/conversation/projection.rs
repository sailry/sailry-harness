use crate::Apply;
use sailry_protocol::{
    conversation::{Change, Draft, Page, Part, Snapshot, Status},
    *,
};
use std::sync::Arc;
pub(super) mod children;
mod questions;

#[cfg(test)]
mod tests;

pub struct Projection {
    node: NodeId,
    session: SessionId,
    generation: u64,
    snapshot: Option<Snapshot>,
    recovery: bool,
}

impl Projection {
    pub fn new(node: NodeId, session: SessionId, generation: u64) -> Self {
        Self {
            node,
            session,
            generation,
            snapshot: None,
            recovery: true,
        }
    }

    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    pub fn reconnect(&mut self, generation: u64) -> Result<(), Fault> {
        if generation <= self.generation {
            return Err(invalid("connection generation must increase"));
        }
        self.generation = generation;
        self.recovery = true;
        Ok(())
    }

    pub(super) fn merge_history(
        &mut self,
        generation: u64,
        before: Option<TurnId>,
        history: conversation::History,
    ) -> Result<Apply, Fault> {
        if generation != self.generation {
            return Ok(Apply::Ignored);
        }
        if self.recovery {
            return Ok(Apply::Recover);
        }
        let Some(snapshot) = self.snapshot.as_mut() else {
            return Ok(Apply::Recover);
        };
        if history.sequence > snapshot.sequence {
            return Ok(Apply::Ignored);
        }
        if history.page.revision != snapshot.page.revision {
            self.recovery = true;
            return Ok(Apply::Recover);
        }
        if !history.missing.is_empty() {
            return Err(invalid("history still contains incomplete turns"));
        }
        if let Some(before) = before {
            if snapshot.page.next_before != Some(before) {
                return Ok(Apply::Ignored);
            }
            let boundary = snapshot
                .page
                .runs
                .iter()
                .find(|run| run.turn == before)
                .ok_or_else(|| invalid("history cursor has no turn"))?
                .sequence;
            if history.page.runs.iter().any(|run| run.sequence >= boundary) {
                return Err(invalid("history page crossed its turn boundary"));
            }
        }
        let next = history.page.next_before;
        super::history::merge(Arc::make_mut(&mut snapshot.page), history.page, true)?;
        if before.is_some() {
            Arc::make_mut(&mut snapshot.page).next_before = next;
        }
        snapshot.missing.clear();
        Ok(Apply::Applied)
    }

    pub fn apply(&mut self, generation: u64, update: Update) -> Result<Apply, Fault> {
        if generation != self.generation {
            return Ok(Apply::Ignored);
        }
        match update {
            Update::ConversationSnapshot(snapshot) => {
                self.check_target(snapshot.node, snapshot.page.session)?;
                if !self.recovery
                    && self
                        .snapshot
                        .as_ref()
                        .is_some_and(|current| snapshot.sequence < current.sequence)
                {
                    return Ok(Apply::Ignored);
                }
                validate_page(&snapshot.page)?;
                let mut missing = std::collections::BTreeSet::new();
                if snapshot.missing.iter().any(|turn| {
                    !missing.insert(*turn)
                        || !snapshot.page.runs.iter().any(|run| run.turn == *turn)
                }) {
                    return Err(invalid(
                        "incomplete history references an unknown or repeated turn",
                    ));
                }
                for draft in &snapshot.drafts {
                    validate_draft(&snapshot.page, draft)?;
                }
                self.snapshot = Some(snapshot);
                self.recovery = false;
                Ok(Apply::Applied)
            }
            Update::ConversationFrame(frame) => {
                self.check_target(frame.node, frame.session)?;
                let Some(snapshot) = self.snapshot.as_mut() else {
                    return Ok(Apply::Recover);
                };
                if frame.sequence <= snapshot.sequence {
                    return Ok(Apply::Ignored);
                }
                if self.recovery || snapshot.sequence.checked_add(1) != Some(frame.sequence) {
                    self.recovery = true;
                    return Ok(Apply::Recover);
                }
                match frame.change {
                    Change::Statistics(statistics) => snapshot.statistics = statistics,
                    Change::Entry(entry) => {
                        let page = Arc::make_mut(&mut snapshot.page);
                        if let Some(previous) =
                            page.entries.iter().find(|previous| previous.id == entry.id)
                        {
                            if previous != &entry {
                                return Err(invalid("conversation event identity changed"));
                            }
                        } else {
                            if entry.sequence == 0
                                || page
                                    .entries
                                    .last()
                                    .is_some_and(|previous| entry.sequence <= previous.sequence)
                            {
                                return Err(invalid("conversation history order changed"));
                            }
                            snapshot
                                .drafts
                                .retain(|draft| draft.turn != entry.turn || draft.id != entry.id);
                            page.entries.push(entry);
                        }
                    }
                    Change::Run(run) => {
                        if !valid_run(&run, self.session) {
                            return Err(invalid("run belongs to another conversation"));
                        }
                        let page = Arc::make_mut(&mut snapshot.page);
                        if let Some(previous) = page
                            .runs
                            .iter_mut()
                            .find(|previous| previous.turn == run.turn)
                        {
                            if previous.revision != run.revision
                                || previous.kind != run.kind
                                || previous.sequence != run.sequence
                                || previous.worktree != run.worktree
                                || previous.origin != run.origin
                            {
                                return Err(invalid("sent turn revision changed"));
                            }
                            *previous = run.clone();
                        } else {
                            if page
                                .runs
                                .iter()
                                .any(|previous| previous.sequence == run.sequence)
                            {
                                return Err(invalid("turn order repeats a sequence"));
                            }
                            page.runs.push(run.clone());
                        }
                        page.runs.sort_by_key(|run| run.sequence);
                        if !matches!(
                            run.status,
                            Status::Queued | Status::Running | Status::Stopping
                        ) {
                            snapshot.drafts.retain(|draft| draft.turn != run.turn);
                        }
                    }
                    Change::Delta(delta) => {
                        validate_draft(&snapshot.page, &delta)?;
                        if let Some(draft) = snapshot
                            .drafts
                            .iter_mut()
                            .find(|draft| draft.id == delta.id && draft.turn == delta.turn)
                        {
                            if draft.author != delta.author || draft.branch != delta.branch {
                                return Err(invalid("streaming event identity changed"));
                            }
                            append(&mut draft.parts, delta.parts);
                        } else {
                            snapshot.drafts.push(delta);
                        }
                    }
                    Change::Queue(queue) => {
                        validate_queue(&queue)?;
                        if queue.revision < snapshot.page.queue.revision {
                            return Err(invalid("queue revision moved backwards"));
                        }
                        if queue.revision == snapshot.page.queue.revision
                            && queue != snapshot.page.queue
                        {
                            return Err(invalid("queue content changed without a revision"));
                        }
                        for item in &queue.items {
                            if let Some(previous) = snapshot
                                .page
                                .queue
                                .items
                                .iter()
                                .find(|previous| previous.turn == item.turn)
                                && (previous.config_revision != item.config_revision
                                    || previous.kind != item.kind
                                    || previous.revision > item.revision
                                    || previous.revision == item.revision
                                        && (previous.preview != item.preview
                                            || previous.truncated != item.truncated))
                            {
                                return Err(invalid("pending input changed without its revision"));
                            }
                        }
                        Arc::make_mut(&mut snapshot.page).queue = queue;
                    }
                    Change::Approval(approval) => {
                        if approval.session != self.session {
                            return Err(invalid("approval belongs to another conversation"));
                        }
                        let page = Arc::make_mut(&mut snapshot.page);
                        validate_approval(page, &approval)?;
                        if let Some(previous) = page
                            .approvals
                            .iter_mut()
                            .find(|previous| previous.id == approval.id)
                        {
                            if previous.turn != approval.turn
                                || previous.entry != approval.entry
                                || previous.index != approval.index
                                || previous.state != conversation::ApprovalState::Pending
                                    && previous.state != approval.state
                            {
                                return Err(invalid(
                                    "approval identity or terminal decision changed",
                                ));
                            }
                            *previous = approval;
                        } else {
                            page.approvals.push(approval);
                        }
                    }
                    Change::Question(question) => {
                        questions::apply(Arc::make_mut(&mut snapshot.page), question)?;
                    }
                    Change::Child(child) => {
                        children::merge(Arc::make_mut(&mut snapshot.page), child, false)?;
                    }
                }
                snapshot.sequence = frame.sequence;
                Ok(Apply::Applied)
            }
            Update::ResetRequired => {
                self.recovery = true;
                Ok(Apply::Recover)
            }
            _ => Err(invalid("unexpected conversation subscription update")),
        }
    }

    fn check_target(&self, node: NodeId, session: SessionId) -> Result<(), Fault> {
        if node == self.node && session == self.session {
            Ok(())
        } else {
            Err(Fault::new(
                ErrorCode::WrongTarget,
                "conversation update belongs to another resource",
            ))
        }
    }
}

pub(super) fn validate_page(page: &Page) -> Result<(), Fault> {
    children::validate(page)?;
    questions::validate(page)?;
    validate_queue(&page.queue)?;
    if page.revision == 0
        || page.entries.iter().any(|entry| entry.sequence == 0)
        || page
            .entries
            .windows(2)
            .any(|pair| pair[0].sequence >= pair[1].sequence)
        || page.runs.iter().any(|run| !valid_run(run, page.session))
        || page
            .runs
            .windows(2)
            .any(|pair| pair[0].sequence >= pair[1].sequence)
    {
        return Err(invalid("conversation snapshot is inconsistent"));
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut turns = std::collections::BTreeSet::new();
    if page.runs.iter().any(|run| !turns.insert(run.turn))
        || page.next_before.is_some_and(|turn| !turns.contains(&turn))
    {
        return Err(invalid(
            "history cursor or turn identities are inconsistent",
        ));
    }
    let mut approvals = std::collections::BTreeSet::new();
    if page
        .approvals
        .iter()
        .any(|approval| approval.session != page.session || !approvals.insert(approval.id))
    {
        return Err(invalid("conversation approvals are inconsistent"));
    }
    for approval in &page.approvals {
        validate_approval(page, approval)?;
    }
    if page.entries.iter().any(|entry| !ids.insert(&entry.id)) {
        return Err(invalid("conversation history repeats an event"));
    }
    Ok(())
}

fn valid_run(run: &conversation::Run, session: SessionId) -> bool {
    run.session == session
        && run.sequence > 0
        && run.origin.is_none_or(|origin| {
            origin != session
                && !matches!(
                    run.status,
                    Status::Queued | Status::Running | Status::Stopping
                )
        })
}

fn validate_approval(page: &Page, approval: &conversation::Approval) -> Result<(), Fault> {
    if approval.entry.is_empty()
        || page
            .entries
            .iter()
            .find(|entry| entry.id == approval.entry)
            .is_some_and(|entry| {
                entry.turn != approval.turn
                    || !matches!(entry.parts.get(approval.index), Some(Part::ToolCall { .. }))
            })
    {
        return Err(invalid("approval does not reference a canonical tool call"));
    }
    Ok(())
}

fn validate_queue(queue: &conversation::Queue) -> Result<(), Fault> {
    let mut turns = std::collections::BTreeSet::new();
    if queue.revision == 0 && (queue.paused || !queue.items.is_empty())
        || queue
            .items
            .iter()
            .any(|item| item.revision == 0 || item.config_revision == 0 || !turns.insert(item.turn))
    {
        return Err(invalid("queue snapshot is inconsistent"));
    }
    Ok(())
}

fn validate_draft(page: &Page, draft: &Draft) -> Result<(), Fault> {
    if draft.id.is_empty()
        || draft
            .parts
            .iter()
            .any(|part| !matches!(part, Part::Text(_) | Part::Thinking(_)))
        || !page.runs.iter().any(|run| {
            run.turn == draft.turn && matches!(run.status, Status::Running | Status::Stopping)
        })
        || page.entries.iter().any(|entry| entry.id == draft.id)
    {
        return Err(invalid("streaming output does not belong to an active run"));
    }
    Ok(())
}

fn append(parts: &mut Vec<Part>, delta: Vec<Part>) {
    for part in delta {
        match (parts.last_mut(), part) {
            (Some(Part::Text(current)), Part::Text(text))
            | (Some(Part::Thinking(current)), Part::Thinking(text)) => current.push_str(&text),
            (_, part) => parts.push(part),
        }
    }
}

pub(super) fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}
