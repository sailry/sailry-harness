use super::*;
use sailry_protocol::conversation::Child;

mod panel;
pub(crate) use panel::{Panel, Tab};
mod presentation;

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;

pub(super) fn status_key(status: Status) -> &'static str {
    match status {
        Status::Queued => "turn_queued",
        Status::Running => "chat_running",
        Status::Stopping => "chat_stopping",
        Status::Completed => "turn_completed",
        Status::Cancelled => "turn_cancelled",
        Status::Interrupted => "chat_interrupted",
        Status::Failed => "turn_failed",
    }
}

impl View {
    pub(crate) fn child(&self, id: SessionId) -> Option<&Child> {
        self.history
            .snapshot
            .as_ref()?
            .page
            .children
            .iter()
            .find(|child| child.run.session == id)
    }

    pub(crate) fn child_session(&self, id: SessionId) -> Option<Session> {
        let child = self.child(id)?;
        let snapshot = self.node.snapshot.as_ref()?;
        snapshot
            .sessions
            .iter()
            .find(|session| {
                session.id == id
                    && session.project == self.binding.project
                    && session.worktree == child.run.worktree
                    && snapshot
                        .worktrees
                        .iter()
                        .any(|tree| tree.id == session.worktree && tree.project == session.project)
                    && session.delegation.as_deref() == Some(&child.origin)
            })
            .cloned()
    }

    pub(in crate::conversation) fn open_child(&mut self, id: SessionId, cx: &mut Context<Self>) {
        if self.child(id).is_some() {
            if self.child_session(id).is_some() {
                if self.error == Some("chat_child_unavailable") {
                    self.error = None;
                    cx.notify();
                }
                cx.emit(Event::Subagent(id));
            } else {
                self.error = Some("chat_child_unavailable");
                cx.notify();
            }
        }
    }

    /// Counts the turn's children as (active, complete, failed, total).
    pub(super) fn child_counts(&self, turn: TurnId) -> Option<(usize, usize, usize, usize)> {
        let children: Vec<_> = self
            .history
            .snapshot
            .as_ref()?
            .page
            .children
            .iter()
            .filter(|child| child.origin.turn == turn)
            .collect();
        if children.is_empty() {
            return None;
        }
        let count = |predicate: fn(Status) -> bool| {
            children
                .iter()
                .filter(|child| predicate(child.run.status))
                .count()
        };
        Some((
            count(|status| matches!(status, Status::Queued | Status::Running | Status::Stopping)),
            count(|status| status == Status::Completed),
            count(|status| status == Status::Failed),
            children.len(),
        ))
    }

    pub(super) fn child_summary(
        &self,
        turn: TurnId,
        children: &[SessionId],
        continuing: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_children(turn, children, continuing, cx)
    }

    pub(super) fn child_activity(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.render_activity(cx)
    }
}
