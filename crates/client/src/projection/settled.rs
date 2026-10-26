use sailry_protocol::{
    Event, SessionId, Snapshot, WorktreeId, activity::Summary, conversation::Status,
};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Changes {
    pub(super) cursors: BTreeMap<WorktreeId, u64>,
}

impl Changes {
    pub(super) fn reset(&mut self, snapshot: &Snapshot) {
        self.cursors = snapshot
            .worktrees
            .iter()
            .map(|tree| (tree.id, snapshot.cursor))
            .collect();
    }

    pub(super) fn observe(&mut self, snapshot: &Snapshot, event: &Event, cursor: u64) {
        match event {
            Event::ToolsCompleted { session } => self.complete(snapshot, *session, cursor),
            Event::ConversationChanged { session, activity } => {
                if finished(snapshot, *session, activity) {
                    self.complete(snapshot, *session, cursor);
                }
            }
            Event::SessionChanged(session) => {
                if finished(snapshot, session.id, &session.activity) {
                    self.cursors.insert(session.worktree, cursor);
                }
            }
            Event::WorktreeRegistered(tree) => {
                self.cursors.insert(tree.id, cursor);
            }
            Event::WorktreeRemoved { id } => {
                self.cursors.remove(id);
            }
            _ => {}
        }
    }

    fn complete(&mut self, snapshot: &Snapshot, session: SessionId, cursor: u64) {
        if let Some(session) = snapshot.sessions.iter().find(|item| item.id == session) {
            let worktree = session
                .activity
                .run
                .as_ref()
                .map_or(session.worktree, |run| run.worktree);
            self.cursors.insert(worktree, cursor);
        }
    }
}

fn finished(snapshot: &Snapshot, session: SessionId, activity: &Summary) -> bool {
    let Some(run) = activity.run.as_ref().filter(|run| {
        !matches!(
            run.status,
            Status::Queued | Status::Running | Status::Stopping
        )
    }) else {
        return false;
    };
    snapshot
        .sessions
        .iter()
        .find(|item| item.id == session)
        .and_then(|session| session.activity.run.as_ref())
        .is_none_or(|previous| previous.turn != run.turn || previous.status != run.status)
}
