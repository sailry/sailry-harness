use super::*;
use serde_json::json;

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "preserves_source_identity",
            (|scenario: &'static str| {
                let command = RequestId::new();
                let task = Task {
                    id: format!("command:{command}"),
                    session: SessionId::new(),
                    turn: TurnId::new(),
                    kind: Kind::Command { id: command },
                    title: "Build".into(),
                    revision: 2,
                    state: State::Pending,
                    outcome: Some(Outcome::TimedOut),
                    started_ms: 1,
                    finished_ms: Some(2),
                    summary: Some("Output remains available on this Node".into()),
                    delivery: None,
                    recovered: false,
                    suppressed: false,
                };
                let value = serde_json::to_value(&task).unwrap();
                assert_eq!(value["state"], "pending", "{scenario}");
                assert_eq!(value["outcome"], "timed_out", "{scenario}");
                assert_eq!(value["kind"]["kind"], "command", "{scenario}");
                assert_eq!(
                    serde_json::from_value::<Task>(value).unwrap(),
                    task,
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "defaults_existing_projections",
            (|scenario: &'static str| {
                let run: crate::conversation::Run = serde_json::from_value(json!({
                    "worktree":crate::WorktreeId::new(),"turn":TurnId::new(),"kind":"task",
                    "session":SessionId::new(),"sequence":1,"revision":1,"status":"completed",
                    "error":null,"started_ms":1,"finished_ms":2,"origin":null
                }))
                .unwrap();
                assert!(!run.notice, "{scenario}");
                assert!(run.notices.is_empty(), "{scenario}");
                let page: crate::conversation::Page = serde_json::from_value(json!({
                    "session":run.session,"revision":1,"entries":[],"runs":[],
                    "queue":{"revision":0,"paused":false,"items":[]},"approvals":[],
                    "questions":[],"children":[],"next_before":null
                }))
                .unwrap();
                assert!(page.background.is_empty(), "{scenario}");
                let pending: crate::conversation::Pending = serde_json::from_value(json!({
                    "turn":run.turn,"kind":"task","revision":1,"config_revision":1,
                    "ready":true,"guidance":null,"preview":"Task","truncated":false,"attachments":[]
                }))
                .unwrap();
                assert!(!pending.notice, "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
