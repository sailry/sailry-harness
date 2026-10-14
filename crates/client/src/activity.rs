//! Activity and notifications derived from the shared Node projection.
mod notifications;
#[cfg(test)]
mod tests;
pub(crate) use notifications::Feed;
pub use notifications::{Id, Inbox, Kind, Notice, Target};

use sailry_protocol::{Session, TerminalId, activity::Waiting, conversation::Status};

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
        Some(Status::Running) => Lane::Running,
        Some(Status::Completed) if session.activity.attention.unread => Lane::Completed,
        Some(Status::Failed | Status::Interrupted) if session.activity.attention.unread => {
            Lane::Failed
        }
        Some(Status::Completed | Status::Failed | Status::Interrupted) => Lane::Idle,
        Some(Status::Queued | Status::Cancelled | Status::Continued) | None => Lane::Idle,
    }
}

/// Display activity includes owned descendants without changing any run's status.
/// The session slice must belong to one execution Node.
pub fn aggregate_lane(session: &Session, sessions: &[Session]) -> Lane {
    let own = lane(session);
    if matches!(own, Lane::Running | Lane::Waiting) {
        return own;
    }
    let scoped: std::collections::BTreeMap<_, _> = sessions
        .iter()
        .filter(|candidate| candidate.project == session.project)
        .map(|candidate| (candidate.id, candidate))
        .collect();
    let mut result = own;
    for child in scoped.values() {
        let activity = lane(child);
        if !matches!(activity, Lane::Running | Lane::Waiting) {
            continue;
        }
        let mut current = *child;
        // Bound ancestry traversal to the supplied graph, including malformed cycles.
        for _ in 0..scoped.len() {
            let Some(origin) = &current.delegation else {
                break;
            };
            if origin.session == session.id {
                if activity == Lane::Running {
                    return Lane::Running;
                }
                result = Lane::Waiting;
                break;
            }
            let Some(parent) = scoped.get(&origin.session) else {
                break;
            };
            current = parent;
        }
    }
    result
}

/// Display precedence shared by activity surfaces: waiting work, unread
/// results, running work, then idle items.
pub fn precedence(lane: Lane) -> u8 {
    match lane {
        Lane::Waiting => 0,
        Lane::Completed | Lane::Failed => 1,
        Lane::Running => 2,
        Lane::Idle => 3,
    }
}

/// One display order for every activity surface. Waiting and unread items put
/// the latest run activity first; running and idle items keep the supplied
/// newest-first creation order. Sessions must belong to one execution Node, and
/// terminal lanes must already apply the controller's read state. Terminals
/// carry no timestamps, so they follow sessions of equal precedence.
pub fn order(sessions: &[Session], terminals: &[(TerminalId, Lane)]) -> Vec<Target> {
    let mut items: Vec<_> = sessions
        .iter()
        .map(|session| {
            let recent = session
                .activity
                .run
                .as_ref()
                .and_then(|run| run.finished_ms.or(run.started_ms));
            (
                precedence(aggregate_lane(session, sessions)),
                recent,
                Target::Session(session.id),
            )
        })
        .chain(
            terminals
                .iter()
                .map(|(id, lane)| (precedence(*lane), None, Target::Terminal(*id))),
        )
        .collect();
    // The sort is stable, so equal keys keep the supplied creation order.
    items.sort_by(|left, right| {
        left.0.cmp(&right.0).then_with(|| {
            if left.0 <= precedence(Lane::Completed) {
                right.1.cmp(&left.1)
            } else {
                std::cmp::Ordering::Equal
            }
        })
    });
    items.into_iter().map(|(_, _, target)| target).collect()
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
