//! Activity and notifications derived from the shared Node projection.
mod notifications;
pub(crate) use notifications::Feed;
pub use notifications::{Id, Inbox, Kind, Notice, Target};

use sailry_protocol::{Session, activity::Waiting, conversation::Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Idle,
    Running,
    Waiting,
    Completed,
    Failed,
}

pub fn lane(session: &Session) -> Lane {
    match session.activity.run.as_ref().map(|run| run.status) {
        Some(Status::Stopping) => Lane::Running,
        Some(Status::Running) if session.activity.waiting.is_some() => Lane::Waiting,
        _ if session.activity.queued > 0 => Lane::Running,
        Some(Status::Queued | Status::Running) => Lane::Running,
        Some(Status::Completed) if session.activity.attention.unread => Lane::Completed,
        Some(Status::Failed | Status::Interrupted) if session.activity.attention.unread => {
            Lane::Failed
        }
        Some(Status::Completed | Status::Failed | Status::Interrupted) => Lane::Idle,
        Some(Status::Cancelled) | None => Lane::Idle,
    }
}

fn attention(waiting: Waiting) -> Kind {
    match waiting {
        Waiting::Approval => Kind::Approval,
        Waiting::Input => Kind::Input,
    }
}

pub fn terminal_lane(info: &sailry_protocol::terminal::Info) -> Option<Lane> {
    use sailry_protocol::terminal::Status;
    match &info.status {
        Status::Running => Some(Lane::Running),
        Status::Exited { code: 0 } => Some(Lane::Completed),
        Status::Exited { .. } | Status::Failed { .. } => Some(Lane::Failed),
        Status::Stopped => Some(Lane::Idle),
        Status::Closed => None,
    }
}

/// OSC-reported work, rather than the lifetime of the shell process.
pub fn terminal_activity(info: &sailry_protocol::terminal::Info) -> Option<Lane> {
    use sailry_protocol::terminal::{Activity, Status};
    if info.status != Status::Running {
        return terminal_lane(info);
    }
    Some(match info.activity.map(|report| report.state) {
        Some(Activity::Working) => Lane::Running,
        Some(Activity::WaitingForInput) => Lane::Waiting,
        Some(Activity::Completed) => Lane::Completed,
        Some(Activity::Failed) => Lane::Failed,
        Some(Activity::Idle) | None => Lane::Idle,
    })
}
