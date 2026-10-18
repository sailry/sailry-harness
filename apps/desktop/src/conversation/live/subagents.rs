use super::*;
use sailry_protocol::conversation::Child;

mod panel;
pub(crate) use panel::{Panel, Tab};
mod presentation;

pub(super) fn status_key(status: Status) -> &'static str {
    match status {
        Status::Queued => "turn_queued",
        Status::Running => "chat_running",
        Status::Stopping => "chat_stopping",
        Status::Completed => "turn_completed",
        Status::Continued => "turn_continued",
        Status::Cancelled => "turn_cancelled",
        Status::Interrupted => "chat_interrupted",
        Status::Failed => "turn_failed",
    }
}

pub(super) fn latest<'a>(children: impl IntoIterator<Item = &'a Child>) -> Vec<&'a Child> {
    let mut selected = BTreeMap::<SessionId, &Child>::new();
    for child in children {
        let current = selected.entry(child.run.session).or_insert(child);
        if rank(child) > rank(current) {
            *current = child;
        }
    }
    let mut children: Vec<_> = selected.into_values().collect();
    children.sort_by_key(|child| child.run.sequence);
    children
}

pub(super) fn rank(child: &Child) -> (bool, bool, u64) {
    (
        matches!(child.run.status, Status::Running | Status::Stopping),
        child.run.status != Status::Queued,
        child.run.sequence,
    )
}

pub(super) fn key(child: &Child) -> String {
    format!(
        "{}-{}-{}-{}",
        child.run.session, child.origin.turn, child.origin.entry, child.origin.index
    )
}

fn running(children: &[Child], session: Option<SessionId>) -> Vec<&Child> {
    latest(
        children
            .iter()
            .filter(|child| Some(child.origin.session) == session),
    )
    .into_iter()
    .filter(|child| child.run.status == Status::Running)
    .collect()
}

#[cfg(test)]
mod tests;

impl View {
    pub(crate) fn child(&self, id: SessionId) -> Option<&Child> {
        self.history
            .snapshot
            .as_ref()?
            .page
            .children
            .iter()
            .filter(|child| child.run.session == id)
            .max_by_key(|child| rank(child))
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
                    && session.delegation.as_ref().is_some_and(|origin| {
                        origin.session == child.origin.session && origin.role == child.origin.role
                    })
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

    fn activity_children(&self) -> Vec<&Child> {
        let Some(snapshot) = &self.history.snapshot else {
            return Vec::new();
        };
        running(&snapshot.page.children, self.session())
    }

    pub(super) fn child_summary(
        &self,
        turn: TurnId,
        children: &[&Child],
        continuing: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_children(turn, children, continuing, cx)
    }

    pub(super) fn child_activity(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.render_activity(cx)
    }
}
