use sailry_protocol::{
    NodeId, NotificationId, ProjectId, SessionId, Snapshot, TerminalId, WorktreeId,
    activity::Summary, conversation::Status, notification::Details,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const LIMIT: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Target {
    Notification(NotificationId),
    Session(SessionId),
    Terminal(TerminalId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Id {
    pub node: NodeId,
    pub cursor: u64,
    pub target: Target,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Kind {
    Notification(sailry_protocol::notification::Kind),
    Completed,
    Failed,
    Approval,
    Input,
}

#[derive(Clone, Debug, Serialize)]
pub struct Notice {
    pub id: Id,
    pub project: Option<ProjectId>,
    pub worktree: Option<WorktreeId>,
    pub title: String,
    pub message: String,
    pub error: Option<sailry_protocol::Fault>,
    pub presentation: Option<sailry_protocol::notification::Presentation>,
    pub kind: Kind,
    pub read: bool,
    pub package: String,
    pub session: Option<SessionId>,
    pub timestamp_ms: Option<i64>,
    pub details: Option<Details>,
}

impl Notice {
    pub fn target(&self) -> Target {
        if let Some(session) = self.session {
            Target::Session(session)
        } else if let Some(Details::Terminal(terminal)) = &self.details {
            Target::Terminal(terminal.id)
        } else {
            self.id.target
        }
    }
}

/// Controller projection and transient attention state. Durable read/dismiss state is
/// authoritative on Node and reconciled from the durable notification snapshot.
#[derive(Default)]
pub struct Inbox {
    notices: Vec<Notice>,
    cursors: BTreeMap<(NodeId, bool), u64>,
}

impl Inbox {
    pub fn notices(&self) -> &[Notice] {
        &self.notices
    }

    pub fn unread(&self) -> usize {
        self.notices.iter().filter(|notice| !notice.read).count()
    }

    pub fn merge(&mut self, node: NodeId, notices: &[Notice]) -> Vec<Notice> {
        self.notices.retain(|entry| {
            entry.id.node != node
                || !matches!(entry.id.target, Target::Notification(_))
                || notices.iter().any(|notice| notice.id == entry.id)
        });
        for notice in notices {
            if notice.id.node == node
                && matches!(notice.id.target, Target::Notification(_))
                && let Some(entry) = self.notices.iter_mut().find(|entry| entry.id == notice.id)
            {
                entry.read = notice.read;
            }
        }
        let mut added = Vec::new();
        let cursors = self.cursors.clone();
        for notice in notices.iter().rev().filter(|notice| {
            let key = (node, matches!(notice.id.target, Target::Notification(_)));
            notice.id.node == node
                && cursors
                    .get(&key)
                    .is_none_or(|cursor| notice.id.cursor > *cursor)
        }) {
            self.notices.insert(0, notice.clone());
            if !notice.read {
                added.push(notice.clone());
            }
            let key = (node, matches!(notice.id.target, Target::Notification(_)));
            self.cursors
                .entry(key)
                .and_modify(|cursor| *cursor = (*cursor).max(notice.id.cursor))
                .or_insert(notice.id.cursor);
        }
        retain_attention(&mut self.notices);
        added
    }

    pub fn mark_read(&mut self, id: Id) {
        if let Some(notice) = self.notices.iter_mut().find(|notice| notice.id == id) {
            notice.read = true;
        }
    }

    /// Durable plugin notices remain visible until Node confirms dismissal.
    pub fn clear_local(&mut self) {
        self.notices
            .retain(|notice| matches!(notice.id.target, Target::Notification(_)));
    }

    pub fn mark_all_read(&mut self) {
        self.notices
            .iter_mut()
            .for_each(|notice| notice.read = true);
    }
}

// Durable history is bounded by each Node's store, not by the aggregate inbox.
fn retain_attention(notices: &mut Vec<Notice>) {
    let mut attention = 0;
    notices.retain(|notice| {
        if matches!(notice.id.target, Target::Notification(_)) {
            true
        } else {
            attention += 1;
            attention <= LIMIT
        }
    });
}

#[derive(Default)]
pub(crate) struct Feed {
    primed: bool,
    sessions: BTreeMap<SessionId, Summary>,
    pub notices: Vec<Notice>,
}

impl Feed {
    // Called for every applied event before watch-channel coalescing. A fast
    // completion cannot disappear when another turn immediately starts.
    pub fn observe(&mut self, snapshot: &Snapshot) {
        self.notices.retain(|notice| match notice.id.target {
            Target::Notification(id) => snapshot.notifications.iter().any(|entry| entry.id == id),
            _ => true,
        });
        for entry in snapshot.notifications.iter().rev() {
            let id = Id {
                node: snapshot.node,
                cursor: entry.sequence,
                target: Target::Notification(entry.id),
            };
            if let Some(notice) = self.notices.iter_mut().find(|notice| notice.id == id) {
                notice.read = entry.read;
            } else {
                let session = entry
                    .content
                    .session
                    .and_then(|id| snapshot.sessions.iter().find(|session| session.id == id));
                let terminal = match &entry.content.details {
                    Some(Details::Terminal(details)) => snapshot
                        .terminals
                        .iter()
                        .find(|terminal| terminal.id == details.id),
                    _ => None,
                };
                let worktree = session
                    .map(|session| session.worktree)
                    .or_else(|| terminal.and_then(|terminal| terminal.worktree));
                let project = session.and_then(|session| session.project).or_else(|| {
                    snapshot
                        .worktrees
                        .iter()
                        .find(|tree| Some(tree.id) == worktree)
                        .and_then(|tree| tree.project)
                });
                let kind = match &entry.content.details {
                    Some(Details::Task(task)) => {
                        if task.status == Status::Completed {
                            Kind::Completed
                        } else {
                            Kind::Failed
                        }
                    }
                    Some(Details::Terminal(terminal)) => {
                        if terminal.failed {
                            Kind::Failed
                        } else {
                            Kind::Completed
                        }
                    }
                    _ => Kind::Notification(entry.content.kind),
                };
                let error = match &entry.content.details {
                    Some(Details::Task(task)) => {
                        task.error.map(|code| sailry_protocol::Fault::new(code, ""))
                    }
                    _ => None,
                };
                self.notices.insert(
                    0,
                    Notice {
                        id,
                        project,
                        worktree,
                        title: entry.content.title.clone(),
                        message: entry.content.message.clone(),
                        error,
                        presentation: entry.content.presentation.clone(),
                        kind,
                        read: entry.read,
                        package: entry.package.clone(),
                        session: entry.content.session,
                        timestamp_ms: Some(entry.timestamp_ms),
                        details: entry.content.details.clone(),
                    },
                );
            }
        }
        for session in &snapshot.sessions {
            if session.delegation.is_some() {
                continue;
            }
            let activity = &session.activity;
            let previous = self.sessions.insert(session.id, activity.clone());
            if !self.primed {
                continue;
            }
            let Some(previous) = previous else { continue };
            let Some(run) = &activity.run else { continue };
            let signature = |summary: &Summary| {
                summary
                    .run
                    .as_ref()
                    .map(|run| (run.turn, run.status, summary.waiting))
            };
            if signature(&previous) == signature(activity)
                || previous
                    .run
                    .as_ref()
                    .is_some_and(|prior| prior.sequence > run.sequence)
            {
                continue;
            }
            let kind = match run.status {
                Status::Running => activity.waiting.map(super::attention),
                _ => None,
            };
            if let Some(kind) = kind {
                self.notices.insert(
                    0,
                    Notice {
                        id: Id {
                            node: snapshot.node,
                            cursor: snapshot.cursor,
                            target: Target::Session(session.id),
                        },
                        project: session.project,
                        worktree: Some(session.worktree),
                        title: activity.title.clone(),
                        message: String::new(),
                        error: None,
                        presentation: None,
                        kind,
                        read: false,
                        package: String::new(),
                        session: Some(session.id),
                        timestamp_ms: None,
                        details: None,
                    },
                );
            }
        }
        self.sessions
            .retain(|id, _| snapshot.sessions.iter().any(|s| s.id == *id));
        retain_attention(&mut self.notices);
        self.primed = true;
    }
}

#[cfg(test)]
mod inbox_tests {
    use super::*;

    #[test]
    fn clearing_preserves_delivery_cursor() {
        let node = NodeId([1; 32]);
        let mut notice = Notice {
            id: Id {
                node,
                cursor: 1,
                target: Target::Session(SessionId::new()),
            },
            project: None,
            worktree: Some(WorktreeId::new()),
            title: "Fixture".into(),
            message: String::new(),
            error: None,
            presentation: None,
            kind: Kind::Completed,
            read: false,
            package: String::new(),
            session: None,
            timestamp_ms: None,
            details: None,
        };
        let mut inbox = Inbox::default();
        assert_eq!(inbox.merge(node, &[notice.clone()]).len(), 1);
        inbox.clear_local();
        assert_eq!(inbox.unread(), 0);
        assert!(inbox.merge(node, &[notice.clone()]).is_empty());
        notice.id.cursor += 1;
        assert_eq!(inbox.merge(node, &[notice]).len(), 1);
        assert_eq!(inbox.unread(), 1);
    }
}
