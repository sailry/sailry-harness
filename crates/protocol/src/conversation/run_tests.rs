use super::*;
use crate::Command;
use serde_json::json;

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "preserves_termination_status",
            (|scenario: &'static str| {
                for (status, value) in [
                    (Status::Completed, "completed"),
                    (Status::Continued, "continued"),
                    (Status::Cancelled, "cancelled"),
                    (Status::Interrupted, "interrupted"),
                ] {
                    assert_eq!(serde_json::to_value(status).unwrap(), value, "{scenario}");
                    assert_eq!(
                        serde_json::from_value::<Status>(json!(value)).unwrap(),
                        status,
                        "{scenario}"
                    );
                }
            }) as fn(&'static str),
        ),
        (
            "preserves_explicit_kind",
            (|scenario: &'static str| {
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
                        guidance: None,
                        joined: None,
                        notice: false,
                        notices: Vec::new(),
                        input: None,
                    };
                    let body = serde_json::to_value(&run).unwrap();
                    assert!(body.get("guidance").is_none(), "{scenario}");
                    assert!(body.get("joined").is_none(), "{scenario}");
                    assert_eq!(body["kind"], name, "{scenario}");
                    assert_eq!(
                        serde_json::from_value::<Run>(body).unwrap(),
                        run,
                        "{scenario}"
                    );
                    let mut guided = run;
                    guided.guidance = Some(TurnId::new());
                    guided.joined = Some(TurnId::new());
                    let body = serde_json::to_value(&guided).unwrap();
                    assert_ne!(body["guidance"], body["joined"], "{scenario}");
                    assert_eq!(
                        serde_json::from_value::<Run>(body).unwrap(),
                        guided,
                        "{scenario}"
                    );
                }
            }),
        ),
        (
            "encodes_context_command",
            (|scenario: &'static str| {
                let session = SessionId::new();
                let command = Command::CompactContext {
                    session,
                    expected_revision: 7,
                    focus: None,
                };
                let body = json!({"kind": "compact_context", "data": {"session": session, "expected_revision": 7}});
                assert_eq!(serde_json::to_value(&command).unwrap(), body, "{scenario}");
                assert_eq!(
                    serde_json::from_value::<Command>(body).unwrap(),
                    command,
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
