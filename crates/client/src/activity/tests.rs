use super::*;
use sailry_protocol::{
    Effort, Permission, ProviderId, SessionConfig, SessionId, TurnId, WorkMode, WorktreeId,
    conversation::{Run, RunKind},
};

fn session(status: Option<Status>) -> Session {
    let mut session = Session {
        id: SessionId::new(),
        archived: false,
        activity: Default::default(),
        project: None,
        worktree: WorktreeId::new(),
        revision: 1,
        config: SessionConfig {
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: Effort::Default,
            mode: WorkMode::Code,
            permission: Permission::Ask,
            credential: None,
        },
        roles: Default::default(),
        profile: None,
        fork: None,
        delegation: None,
    };
    session.activity.queued = 2;
    session.activity.run = status.map(|status| Run {
        worktree: session.worktree,
        turn: TurnId::new(),
        kind: RunKind::Task,
        session: session.id,
        sequence: 1,
        revision: 1,
        status,
        error: None,
        started_ms: None,
        finished_ms: None,
        origin: None,
        continuation: None,
    });
    session
}

#[test]
fn queued_items_do_not_imply_running() {
    for status in [None, Some(Status::Queued), Some(Status::Cancelled)] {
        let session = session(status);
        assert_eq!(lane(&session), Lane::Idle);
        assert_eq!(session.activity.queued, 2);
    }
}

#[test]
fn terminal_attention_survives_queued_items() {
    for (status, expected) in [
        (Status::Completed, Lane::Completed),
        (Status::Failed, Lane::Failed),
        (Status::Interrupted, Lane::Failed),
    ] {
        let mut session = session(Some(status));
        session.activity.attention.unread = true;
        assert_eq!(lane(&session), expected);
        session.activity.attention.unread = false;
        assert_eq!(lane(&session), Lane::Idle);
    }
}

#[test]
fn active_execution_keeps_running_lane() {
    for status in [Status::Running, Status::Stopping] {
        let mut session = session(Some(status));
        for queued in [0, 2] {
            session.activity.queued = queued;
            assert_eq!(lane(&session), Lane::Running);
        }
    }
}

#[test]
fn waiting_execution_keeps_waiting_lane() {
    for waiting in [Waiting::Approval, Waiting::Input] {
        let mut session = session(Some(Status::Running));
        session.activity.waiting = Some(waiting);
        assert_eq!(lane(&session), Lane::Waiting);
        session.activity.run.as_mut().unwrap().status = Status::Stopping;
        assert_eq!(lane(&session), Lane::Running);
    }
}
