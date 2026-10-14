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
        input: None,
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

fn child(parent: &Session, status: Status) -> Session {
    let mut child = session(Some(status));
    child.project = parent.project;
    child.delegation = Some(Box::new(sailry_protocol::Delegation {
        session: parent.id,
        turn: parent.activity.run.as_ref().unwrap().turn,
        entry: "delegated-call".into(),
        index: 0,
        role: None,
    }));
    child
}

#[test]
fn child_activity_preserves_parent_completion() {
    let mut parent = session(Some(Status::Completed));
    parent.activity.attention.unread = true;
    for (status, expected) in [
        (Status::Running, Lane::Running),
        (Status::Stopping, Lane::Running),
        (Status::Queued, Lane::Completed),
        (Status::Continued, Lane::Completed),
        (Status::Completed, Lane::Completed),
        (Status::Failed, Lane::Completed),
        (Status::Cancelled, Lane::Completed),
        (Status::Interrupted, Lane::Completed),
    ] {
        let child = child(&parent, status);
        let sessions = [parent.clone(), child];
        assert_eq!(aggregate_lane(&parent, &sessions), expected);
        assert_eq!(lane(&parent), Lane::Completed);
        assert_eq!(
            parent.activity.run.as_ref().unwrap().status,
            Status::Completed
        );
    }
}

#[test]
fn nested_activity_survives_closed_intermediate_run() {
    let parent = session(Some(Status::Completed));
    let intermediate = child(&parent, Status::Completed);
    let mut nested = child(&intermediate, Status::Running);
    let mut sessions = vec![parent.clone(), intermediate, nested.clone()];
    // Delegated work may use another registered worktree in the same project.
    assert_ne!(nested.worktree, parent.worktree);
    assert_eq!(aggregate_lane(&parent, &sessions), Lane::Running);
    nested.activity.waiting = Some(Waiting::Approval);
    sessions[2] = nested;
    assert_eq!(aggregate_lane(&parent, &sessions), Lane::Waiting);
    sessions[2].activity.run.as_mut().unwrap().status = Status::Completed;
    assert_eq!(aggregate_lane(&parent, &sessions), Lane::Idle);
}

#[test]
fn own_active_run_keeps_its_lane() {
    let mut parent = session(Some(Status::Running));
    parent.activity.waiting = Some(Waiting::Input);
    let child = child(&parent, Status::Running);
    assert_eq!(
        aggregate_lane(&parent, &[parent.clone(), child]),
        Lane::Waiting
    );
}

#[test]
fn ignores_other_projects_and_unrelated_ancestry() {
    let parent = session(Some(Status::Completed));
    let unrelated = session(Some(Status::Completed));
    let stranger = child(&unrelated, Status::Running);
    let mut misplaced = child(&parent, Status::Running);
    misplaced.project = Some(sailry_protocol::ProjectId::new());
    let mut cyclic = child(&parent, Status::Running);
    cyclic.delegation.as_mut().unwrap().session = cyclic.id;
    let sessions = [parent.clone(), stranger, misplaced, cyclic];
    assert_eq!(aggregate_lane(&parent, &sessions), Lane::Idle);
}

mod order {
    use super::*;
    use sailry_protocol::TerminalId;

    fn finished(status: Status, unread: bool, at: Option<i64>) -> Session {
        let mut session = session(Some(status));
        session.activity.attention.unread = unread;
        session.activity.run.as_mut().unwrap().finished_ms = at;
        session
    }

    #[test]
    fn attention_precedes_running_and_idle() {
        let idle = session(None);
        let running = session(Some(Status::Running));
        let completed = finished(Status::Completed, true, Some(10));
        let mut waiting = session(Some(Status::Running));
        waiting.activity.waiting = Some(Waiting::Approval);
        let terminal = TerminalId::new();
        let sessions = [
            idle.clone(),
            running.clone(),
            completed.clone(),
            waiting.clone(),
        ];
        assert_eq!(
            order(&sessions, &[(terminal, Lane::Running)]),
            [
                Target::Session(waiting.id),
                Target::Session(completed.id),
                Target::Session(running.id),
                Target::Terminal(terminal),
                Target::Session(idle.id),
            ]
        );
    }

    #[test]
    fn results_put_latest_activity_first() {
        let older = finished(Status::Completed, true, Some(10));
        let failed = finished(Status::Failed, true, Some(30));
        let unknown = finished(Status::Completed, true, None);
        let mut started = finished(Status::Completed, true, None);
        started.activity.run.as_mut().unwrap().started_ms = Some(20);
        let sessions = [
            unknown.clone(),
            older.clone(),
            started.clone(),
            failed.clone(),
        ];
        assert_eq!(
            order(&sessions, &[]),
            [failed.id, started.id, older.id, unknown.id].map(Target::Session)
        );
    }

    #[test]
    fn work_keeps_creation_order() {
        let mut newest = session(Some(Status::Running));
        newest.activity.run.as_mut().unwrap().started_ms = Some(10);
        let mut oldest = session(Some(Status::Running));
        oldest.activity.run.as_mut().unwrap().started_ms = Some(20);
        let read = finished(Status::Completed, false, Some(30));
        let idle = session(None);
        let sessions = [newest.clone(), oldest.clone(), read.clone(), idle.clone()];
        assert_eq!(
            order(&sessions, &[]),
            [newest.id, oldest.id, read.id, idle.id].map(Target::Session)
        );
    }
}
