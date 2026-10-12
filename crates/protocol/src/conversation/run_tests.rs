use super::*;
use crate::Command;
use serde_json::json;

#[test]
fn preserves_termination_status() {
    for (status, value) in [
        (Status::Completed, "completed"),
        (Status::Continued, "continued"),
        (Status::Cancelled, "cancelled"),
        (Status::Interrupted, "interrupted"),
    ] {
        assert_eq!(serde_json::to_value(status).unwrap(), value);
        assert_eq!(
            serde_json::from_value::<Status>(json!(value)).unwrap(),
            status
        );
    }
    assert_eq!(crate::VERSION, 1);
}

#[test]
fn preserves_explicit_kind() {
    for (kind, name) in [(RunKind::Task, "task"), (RunKind::Compaction, "compaction")] {
        let run = Run {
            worktree: crate::WorktreeId::new(),
            turn: TurnId::new(),
            kind,
            session: SessionId::new(),
            sequence: 1,
            revision: 1,
            status: Status::Queued,
            error: None,
            started_ms: None,
            finished_ms: None,
            origin: None,
            continuation: None,
            input: None,
        };
        let body = serde_json::to_value(&run).unwrap();
        assert_eq!(body["kind"], name);
        assert_eq!(serde_json::from_value::<Run>(body).unwrap(), run);
    }
}

#[test]
fn encodes_context_command() {
    let session = SessionId::new();
    let command = Command::CompactContext {
        session,
        expected_revision: 7,
    };
    let body =
        json!({"kind": "compact_context", "data": {"session": session, "expected_revision": 7}});
    assert_eq!(serde_json::to_value(&command).unwrap(), body);
    assert_eq!(serde_json::from_value::<Command>(body).unwrap(), command);
    assert_eq!(crate::VERSION, 1);
}
