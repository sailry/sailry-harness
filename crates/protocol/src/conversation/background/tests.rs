use super::*;
use serde_json::json;

#[test]
fn preserves_source_identity() {
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
    assert_eq!(value["state"], "pending");
    assert_eq!(value["outcome"], "timed_out");
    assert_eq!(value["kind"]["kind"], "command");
    assert_eq!(serde_json::from_value::<Task>(value).unwrap(), task);
    assert_eq!(
        Settings::default(),
        Settings {
            revision: 0,
            resume: true,
        }
    );
    assert!(
        serde_json::from_value::<Settings>(json!({"revision":0,"resume":true,"extra":false}))
            .is_err()
    );
}

#[test]
fn round_trips_continuation_preferences() {
    let settings: Settings = serde_json::from_value(json!({"revision":4,"resume":false})).unwrap();
    assert!(!settings.resume);
    assert_eq!(settings.revision, 4);
    assert_eq!(
        serde_json::from_value::<Settings>(serde_json::to_value(&settings).unwrap()).unwrap(),
        settings
    );
}

#[test]
fn defaults_existing_projections() {
    let run: crate::conversation::Run = serde_json::from_value(json!({
        "worktree":crate::WorktreeId::new(),"turn":TurnId::new(),"kind":"task",
        "session":SessionId::new(),"sequence":1,"revision":1,"status":"completed",
        "error":null,"started_ms":1,"finished_ms":2,"origin":null
    }))
    .unwrap();
    assert!(!run.notice);
    assert!(run.notices.is_empty());
    let page: crate::conversation::Page = serde_json::from_value(json!({
        "session":run.session,"revision":1,"entries":[],"runs":[],
        "queue":{"revision":0,"paused":false,"items":[]},"approvals":[],
        "questions":[],"children":[],"next_before":null
    }))
    .unwrap();
    assert!(page.background.is_empty());
    let pending: crate::conversation::Pending = serde_json::from_value(json!({
        "turn":run.turn,"kind":"task","revision":1,"config_revision":1,
        "ready":true,"guidance":null,"preview":"Task","truncated":false,"attachments":[]
    }))
    .unwrap();
    assert!(!pending.notice);
}
