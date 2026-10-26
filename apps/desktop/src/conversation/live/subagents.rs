use super::*;
use children::{active as is_running, key, latest, running};
use sailry_client::conversation::children;
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

impl View {
    pub(crate) fn child(&self, id: SessionId) -> Option<&Child> {
        children::selected(&self.history.snapshot.as_ref()?.page.children, id)
    }

    pub(crate) fn child_session(&self, id: SessionId) -> Option<Session> {
        let child = self.child(id)?;
        let snapshot = self.node.snapshot.as_ref()?;
        children::session(snapshot, self.binding.project, child).cloned()
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
