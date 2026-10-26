use super::*;
use core::prelude::v1::test;
use sailry_protocol::conversation::{Run, RunKind};

#[test]
fn isolates_turn_activity() {
    let run = Run {
        worktree: sailry_protocol::WorktreeId::new(),
        turn: TurnId::new(),
        kind: RunKind::Task,
        session: sailry_protocol::SessionId::new(),
        sequence: 1,
        revision: 1,
        status: Status::Completed,
        error: None,
        origin: None,
        continuation: None,
        joined: None,
        notice: false,
        notices: Vec::new(),
        input: None,
        started_ms: Some(1000),
        finished_ms: Some(2000),
    };
    let next = Run {
        turn: TurnId::new(),
        sequence: 2,
        status: Status::Running,
        finished_ms: None,
        ..run.clone()
    };
    let mut page = Page {
        session: run.session,
        revision: 1,
        entries: vec![],
        runs: vec![run.clone(), next.clone()],
        queue: Default::default(),
        approvals: vec![],
        questions: vec![],
        children: vec![],
        background: vec![],
        next_before: None,
    };
    assert!(!active(&page, run.turn));
    assert!(active(&page, next.turn));
    assert!(!active(&page, TurnId::new()));
    for status in [
        Status::Queued,
        Status::Completed,
        Status::Continued,
        Status::Failed,
        Status::Cancelled,
        Status::Interrupted,
    ] {
        page.runs[1].status = status;
        assert!(!active(&page, next.turn));
    }
    page.runs[1].status = Status::Stopping;
    assert!(active(&page, next.turn));
    assert!(!active(&page, run.turn));
}
