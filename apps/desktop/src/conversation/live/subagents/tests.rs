use super::*;
use core::prelude::v1::test;
use sailry_protocol::conversation::{Run, RunKind};

fn child(session: SessionId, status: Status) -> Child {
    Child {
        run: Run {
            worktree: WorktreeId::new(),
            turn: TurnId::new(),
            kind: RunKind::Task,
            session: SessionId::new(),
            sequence: 1,
            revision: 1,
            status,
            error: None,
            started_ms: None,
            finished_ms: None,
            origin: None,
            continuation: None,
            notice: false,
            notices: vec![],
            input: None,
        },
        origin: sailry_protocol::Delegation {
            session,
            turn: TurnId::new(),
            entry: "spawn".into(),
            index: 0,
            role: None,
        },
        name: None,
    }
}

#[test]
fn lists_only_running_children() {
    let session = SessionId::new();
    let mut children: Vec<_> = [
        Status::Queued,
        Status::Running,
        Status::Stopping,
        Status::Completed,
        Status::Continued,
        Status::Cancelled,
        Status::Interrupted,
        Status::Failed,
    ]
    .into_iter()
    .map(|status| child(session, status))
    .collect();
    let expected = children[1].run.turn;
    // Completed siblings from the same spawn call must not join its active tray.
    children[5].origin.turn = children[1].origin.turn;
    children.push(child(SessionId::new(), Status::Running));
    let visible = running(&children, Some(session));
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].run.turn, expected);
    children[1].run.status = Status::Cancelled;
    assert!(running(&children, Some(session)).is_empty());
    assert!(running(&children, None).is_empty());
}
