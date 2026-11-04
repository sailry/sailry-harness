use super::*;
use core::prelude::v1::test;
use sailry_protocol::{SessionId, TurnId};

fn run() -> Run {
    Run {
        worktree: sailry_protocol::WorktreeId::new(),
        turn: TurnId::new(),
        kind: RunKind::Task,
        session: SessionId::new(),
        sequence: 1,
        revision: 1,
        status: Status::Running,
        error: None,
        origin: None,
        continuation: None,
        guidance: None,
        joined: None,
        notice: false,
        notices: Vec::new(),
        input: None,
        started_ms: Some(1000),
        finished_ms: None,
    }
}

#[test]
fn execution_phases_and_timing() {
    for (scenario, check) in [
        (
            "timestamp_dates",
            (|_scenario: &str| {
                use chrono::TimeZone;
                let now = chrono::Local
                    .with_ymd_and_hms(2026, 9, 14, 20, 6, 3)
                    .unwrap();
                assert_eq!(timestamp(now, now), "20:06", "timestamp dates");
                assert_eq!(
                    timestamp(now - chrono::Duration::days(1), now),
                    "09-13 20:06",
                    "timestamp dates"
                );
                let previous = chrono::Local
                    .with_ymd_and_hms(2025, 9, 14, 20, 6, 3)
                    .unwrap();
                assert_eq!(
                    timestamp(previous, now),
                    "2025-09-14 20:06",
                    "timestamp dates"
                );
            }) as fn(&str),
        ),
        (
            "elapsed_execution_time",
            (|_scenario: &str| {
                let mut run = run();
                for (now, expected) in [
                    (500, "0s"),
                    (2000, "1s"),
                    (61000, "1m00s"),
                    (3602000, "1h00m01s"),
                ] {
                    assert_eq!(
                        elapsed(&run, now).as_deref(),
                        Some(expected),
                        "elapsed execution time"
                    );
                }
                run.status = Status::Queued;
                run.started_ms = None;
                assert_eq!(elapsed(&run, 900000), None, "elapsed execution time");
                run.status = Status::Running;
                assert_eq!(elapsed(&run, 900000), None, "elapsed execution time");
                run.started_ms = Some(1000);
                run.status = Status::Completed;
                run.finished_ms = Some(71000);
                assert_eq!(
                    elapsed(&run, 900000).as_deref(),
                    Some("1m10s"),
                    "elapsed execution time"
                );
                assert!(finished(&run).is_some(), "elapsed execution time");
                run.status = Status::Interrupted;
                run.finished_ms = None;
                assert_eq!(elapsed(&run, 900000), None, "elapsed execution time");
                assert_eq!(finished(&run), None, "elapsed execution time");
            }),
        ),
        (
            "active_phase_lifecycle",
            (|_scenario: &str| {
                let mut run = run();
                let mut page = Page {
                    session: run.session,
                    revision: 1,
                    entries: vec![].into(),
                    runs: vec![],
                    queue: Default::default(),
                    approvals: vec![],
                    questions: vec![],
                    children: vec![],
                    background: Vec::new(),
                    next_before: None,
                };
                assert_eq!(
                    phase(&run, &page, &[]),
                    Some("turn_awaiting_response"),
                    "active phase lifecycle"
                );
                let reasoning = [Block::Thinking(
                    "reasoning".into(),
                    "Thinking details".into(),
                )];
                assert_eq!(
                    phase(&run, &page, &reasoning),
                    Some("turn_thinking"),
                    "active phase lifecycle"
                );
                let text = [Block::Text("response".into(), "Partial answer".into())];
                assert_eq!(
                    phase(&run, &page, &text),
                    Some("turn_generating"),
                    "active phase lifecycle"
                );
                let activity = [Block::Tools(vec![])];
                assert!(
                    continuing_tools(&run, &page, &activity),
                    "active phase lifecycle"
                );
                assert_eq!(
                    phase(&run, &page, &activity),
                    None,
                    "active phase lifecycle"
                );
                let next = [
                    Block::Tools(vec![]),
                    Block::Thinking("next".into(), "New reasoning".into()),
                ];
                assert!(
                    !continuing_tools(&run, &page, &next),
                    "active phase lifecycle"
                );
                assert_eq!(
                    phase(&run, &page, &next),
                    Some("turn_thinking"),
                    "active phase lifecycle"
                );
                let retry = [
                    Block::Tools(vec![]),
                    Block::Retry(
                        "retry".into(),
                        sailry_protocol::conversation::recovery::Retry::from_resource(
                            &serde_json::json!({"type":"model_retry","attempt":1,"limit":5}),
                        )
                        .unwrap(),
                    ),
                ];
                assert!(
                    !continuing_tools(&run, &page, &retry),
                    "active phase lifecycle"
                );
                assert_eq!(phase(&run, &page, &retry), None, "active phase lifecycle");
                // Finished tools hand the phase to automatic compaction.
                let compacting = [Block::Tools(vec![]), Block::Compacting(1_500)];
                assert!(
                    !continuing_tools(&run, &page, &compacting),
                    "active phase lifecycle"
                );
                assert_eq!(
                    super::compacting(&compacting),
                    Some(1_500),
                    "active phase lifecycle"
                );
                assert_eq!(
                    phase(&run, &page, &compacting),
                    Some("turn_compacting"),
                    "active phase lifecycle"
                );
                page.questions
                    .push(sailry_protocol::conversation::question::Question {
                        id: sailry_protocol::QuestionId::new(),
                        session: run.session,
                        turn: run.turn,
                        entry: "question".into(),
                        index: 0,
                        state: sailry_protocol::conversation::question::State::Pending,
                    });
                assert_eq!(phase(&run, &page, &[]), None, "active phase lifecycle");
                assert_eq!(phase(&run, &page, &text), None, "active phase lifecycle");
                page.questions[0].turn = TurnId::new();
                assert_eq!(
                    phase(&run, &page, &[]),
                    Some("turn_awaiting_response"),
                    "active phase lifecycle"
                );
                page.questions.clear();
                page.approvals
                    .push(sailry_protocol::conversation::Approval {
                        id: sailry_protocol::ApprovalId::new(),
                        session: run.session,
                        turn: run.turn,
                        entry: "tool".into(),
                        index: 0,
                        state: sailry_protocol::conversation::ApprovalState::Pending,
                        source: sailry_protocol::conversation::ApprovalSource::User,
                        choice: None,
                        reason: None,
                        prefix: None,
                        can_remember: false,
                        startup: None,
                        ssh: None,
                        access: None,
                    });
                assert_eq!(phase(&run, &page, &[]), None, "active phase lifecycle");
                assert_eq!(phase(&run, &page, &text), None, "active phase lifecycle");
                page.approvals[0].turn = TurnId::new();
                assert_eq!(
                    phase(&run, &page, &[]),
                    Some("turn_awaiting_response"),
                    "active phase lifecycle"
                );
                page.approvals[0].turn = run.turn;
                page.approvals[0].state = sailry_protocol::conversation::ApprovalState::Approved;
                assert_eq!(
                    phase(&run, &page, &[]),
                    Some("turn_awaiting_response"),
                    "active phase lifecycle"
                );
                assert_eq!(
                    phase(&run, &page, &text),
                    Some("turn_generating"),
                    "active phase lifecycle"
                );
                for status in [
                    Status::Completed,
                    Status::Continued,
                    Status::Failed,
                    Status::Cancelled,
                    Status::Interrupted,
                ] {
                    run.status = status;
                    if status == Status::Cancelled {
                        assert_eq!(
                            super::status(&run, true),
                            "turn_cancelled",
                            "active phase lifecycle"
                        );
                    } else if status == Status::Continued {
                        assert_eq!(
                            super::status(&run, true),
                            "turn_continued",
                            "active phase lifecycle"
                        );
                    }
                    assert!(
                        !continuing_tools(&run, &page, &activity),
                        "active phase lifecycle"
                    );
                    assert_eq!(phase(&run, &page, &[]), None, "active phase lifecycle");
                    assert_eq!(phase(&run, &page, &text), None, "active phase lifecycle");
                }
            }),
        ),
        (
            "excludes_later_child_calls",
            (|_scenario: &str| {
                let run = run();
                let original = sailry_protocol::conversation::Child {
                    run: Run {
                        status: Status::Completed,
                        ..run.clone()
                    },
                    origin: sailry_protocol::Delegation {
                        session: run.session,
                        turn: run.turn,
                        entry: "original".into(),
                        index: 0,
                        role: None,
                    },
                    name: None,
                };
                let mut reused = original.clone();
                reused.origin.entry = "later".into();
                reused.run.turn = TurnId::new();
                reused.run.status = Status::Running;
                let page = Page {
                    session: run.session,
                    revision: 1,
                    entries: vec![].into(),
                    runs: vec![],
                    queue: Default::default(),
                    approvals: vec![],
                    questions: vec![],
                    children: vec![original.clone(), reused],
                    background: Vec::new(),
                    next_before: None,
                };
                let blocks = [
                    Block::Children(vec![&original]),
                    Block::Text("report".into(), "Earlier call report".into()),
                ];
                assert_eq!(
                    phase(&run, &page, &blocks),
                    Some("turn_generating"),
                    "excludes later child calls"
                );
            }),
        ),
        (
            "completion_precedence",
            (|_scenario: &str| {
                let mut run = run();
                assert_eq!(
                    status(&run, false),
                    "chat_status_unsynced",
                    "completion precedence"
                );
                assert_eq!(status(&run, true), "chat_running", "completion precedence");
                for terminal in [
                    Status::Completed,
                    Status::Cancelled,
                    Status::Failed,
                    Status::Interrupted,
                ] {
                    run.status = terminal;
                    assert_eq!(
                        status(&run, false),
                        super::super::super::subagents::status_key(terminal),
                        "completion precedence"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
