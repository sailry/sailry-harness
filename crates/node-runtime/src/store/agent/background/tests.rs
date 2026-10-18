use super::*;
use crate::store::agent::tests::fixture::Fixture;
use sailry_protocol::process::{Access, Capture, Info, Snapshot};
use serde_json::Value;
use std::collections::BTreeSet;

fn pending(fixture: &Fixture, turn: TurnId, output: &str) -> String {
    let id = RequestId::new();
    let mut record = Record {
        task: Task {
            id: format!("command:{id}"),
            session: fixture.session,
            turn,
            kind: Kind::Command { id },
            title: "Background fixture".into(),
            revision: 0,
            state: State::Pending,
            outcome: Some(Outcome::Completed),
            started_ms: 1,
            finished_ms: Some(2),
            summary: None,
            delivery: None,
            recovered: false,
            suppressed: false,
        },
        stdout: Some(output.into()),
        stderr: None,
        response: None,
        elapsed_ms: Some(1),
        sequence: next_sequence(&fixture.database.connection).unwrap(),
    };
    save(&fixture.database.connection, &mut record).unwrap();
    record.task.id
}

fn schedule(fixture: &Fixture) -> Option<QueuedTurn> {
    super::schedule(
        &fixture.database.connection,
        fixture.database.node,
        fixture.session,
    )
    .unwrap()
}

fn claim(fixture: &mut Fixture) -> Invocation {
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap()
}

fn event(invocation: &Invocation) -> AdkEvent {
    let mut event = AdkEvent::new("background-fixture");
    event.author = "user".into();
    event.set_content(
        adk_core::Content::new("user").with_text(invocation.notice_input.as_ref().unwrap()),
    );
    event.provider_metadata.insert(
        super::super::sessions::INPUT_TURN.into(),
        invocation.turn.id.to_string(),
    );
    event
        .provider_metadata
        .insert(super::super::sessions::INPUT_SOURCE.into(), "notice".into());
    event.provider_metadata.insert(
        INPUT_IDS.into(),
        serde_json::to_string(
            &invocation
                .notices
                .iter()
                .map(|notice| &notice.id)
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    );
    event
}

fn append(fixture: &mut Fixture, invocation: &Invocation, event: AdkEvent) {
    super::super::history::append(
        &mut fixture.database,
        fixture.session,
        invocation.turn.id,
        event,
        &fixture.events,
    )
    .unwrap();
}

#[test]
fn delivers_all_bounded_batches() {
    for output in ["Result".to_owned(), "\n".repeat(2048)] {
        let mut fixture = Fixture::new();
        let source = fixture.completed(&["Started work"]);
        let ids: BTreeSet<_> = (0..17)
            .map(|_| pending(&fixture, source, &output))
            .collect();
        let queued = schedule(&fixture).unwrap();
        let reserved = runs::get(&fixture.database.connection, queued.id).unwrap();
        assert!(reserved.notice);
        assert!(!reserved.notices.is_empty());
        let mut delivered = BTreeSet::new();
        let mut batches = 0;
        while !queue::read(&fixture.database.connection, fixture.session)
            .unwrap()
            .items
            .is_empty()
        {
            let invocation = claim(&mut fixture);
            assert!(invocation.notice);
            assert!(!invocation.notices.is_empty());
            assert!(invocation.notices.len() <= 16);
            let input = invocation.notice_input.as_ref().unwrap();
            assert!(input.len() <= 32 * 1024);
            let value: Value =
                serde_json::from_str(input.lines().skip(1).collect::<String>().as_str()).unwrap();
            assert_eq!(value.as_array().unwrap().len(), invocation.notices.len());
            for notice in &invocation.notices {
                let record = get(&fixture.database.connection, &notice.id)
                    .unwrap()
                    .unwrap();
                assert_eq!(record.task.state, State::Pending);
                assert_eq!(record.task.delivery, Some(invocation.turn.id));
                assert!(delivered.insert(notice.id.clone()));
            }
            append(&mut fixture, &invocation, event(&invocation));
            for notice in &invocation.notices {
                assert_eq!(
                    get(&fixture.database.connection, &notice.id)
                        .unwrap()
                        .unwrap()
                        .task
                        .state,
                    State::Delivered
                );
            }
            runs::finish(
                &mut fixture.database,
                invocation.turn.id,
                Status::Completed,
                None,
                &fixture.events,
            )
            .unwrap();
            batches += 1;
        }
        assert!(batches >= 2);
        assert_eq!(delivered, ids);
        assert!(schedule(&fixture).is_none());
    }
}

#[test]
fn cancelling_reservation_prevents_rescheduling() {
    let mut fixture = Fixture::new();
    let source = fixture.completed(&["Started work"]);
    let id = pending(&fixture, source, "Result");
    let queued = schedule(&fixture).unwrap();
    assert_eq!(
        get(&fixture.database.connection, &id)
            .unwrap()
            .unwrap()
            .task
            .delivery,
        Some(queued.id)
    );
    runs::stop(&fixture.database.connection, queued.id).unwrap();
    let record = get(&fixture.database.connection, &id).unwrap().unwrap();
    assert_eq!(record.task.state, State::Pending);
    assert_eq!(record.task.delivery, None);
    assert!(record.task.suppressed);
    assert!(schedule(&fixture).is_none());
    let newer = pending(&fixture, source, "Later result");
    let next = schedule(&fixture).unwrap();
    let run = runs::get(&fixture.database.connection, next.id).unwrap();
    assert_eq!(
        run.notices
            .iter()
            .map(|notice| &notice.id)
            .collect::<Vec<_>>(),
        vec![&newer]
    );
    assert_eq!(
        get(&fixture.database.connection, &id)
            .unwrap()
            .unwrap()
            .task
            .delivery,
        None
    );
}

#[test]
fn recovers_only_unconfirmed_delivery() {
    for confirmed in [false, true] {
        let mut fixture = Fixture::new();
        let source = fixture.completed(&["Started work"]);
        let id = pending(&fixture, source, "Durable result");
        schedule(&fixture).unwrap();
        let invocation = claim(&mut fixture);
        if confirmed {
            append(&mut fixture, &invocation, event(&invocation));
        }
        runs::recover(&fixture.database.connection).unwrap();
        let record = get(&fixture.database.connection, &id).unwrap().unwrap();
        if confirmed {
            assert_eq!(record.task.state, State::Delivered);
            assert_eq!(record.task.delivery, Some(invocation.turn.id));
        } else {
            assert_eq!(record.task.state, State::Pending);
            assert_eq!(record.task.delivery, None);
            assert!(record.task.recovered && record.task.suppressed);
        }
        assert!(schedule(&fixture).is_none());
    }
}

#[test]
fn rejects_forged_canonical_confirmation() {
    let mut fixture = Fixture::new();
    let source = fixture.completed(&["Started work"]);
    let id = pending(&fixture, source, "Durable result");
    schedule(&fixture).unwrap();
    let invocation = claim(&mut fixture);
    let mut forged = event(&invocation);
    forged.set_content(adk_core::Content::new("user").with_text("Different evidence"));
    assert!(
        super::super::history::append(
            &mut fixture.database,
            fixture.session,
            invocation.turn.id,
            forged.clone(),
            &fixture.events
        )
        .is_err()
    );
    assert_eq!(
        get(&fixture.database.connection, &id)
            .unwrap()
            .unwrap()
            .task
            .state,
        State::Pending
    );
    let count: i64 = fixture
        .database
        .connection
        .query_row(
            "SELECT count(*) FROM agent_events WHERE id=?1",
            [&forged.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    let canonical = event(&invocation);
    append(&mut fixture, &invocation, canonical.clone());
    let revision = get(&fixture.database.connection, &id)
        .unwrap()
        .unwrap()
        .task
        .revision;
    append(&mut fixture, &invocation, canonical);
    assert_eq!(
        get(&fixture.database.connection, &id)
            .unwrap()
            .unwrap()
            .task
            .revision,
        revision
    );
}

fn command(fixture: &Fixture, turn: TurnId) -> (Request, Snapshot) {
    let request = Request::new(
        fixture.database.node,
        Command::RunCommand {
            turn,
            command: "fixture command".into(),
            access: Access::Workspace,
            description: None,
            cwd: ".".into(),
            timeout_ms: 1000,
            background: true,
            pty: false,
            attachments: vec![],
        },
    );
    command_started(&fixture.database.connection, &request)
        .unwrap()
        .unwrap();
    let snapshot = Snapshot {
        info: Info {
            id: request.id,
            session: fixture.session,
            turn,
            command: "fixture command".into(),
            access: Access::Workspace,
            pty: false,
            cwd: ".".into(),
            started_ms: 1,
            status: process::Status::Finished(process::Outcome::Exited(0)),
            services: vec![],
        },
        stdout: Capture {
            text: "Result".into(),
            ..Default::default()
        },
        stderr: Capture::default(),
    };
    (request, snapshot)
}

#[test]
fn terminal_read_wins_late_completion() {
    let mut fixture = Fixture::new();
    let queued = fixture.queued();
    runs::start(&fixture.database.connection, queued).unwrap();
    let invocation = claim(&mut fixture);
    let (request, snapshot) = command(&fixture, invocation.turn.id);
    let changed =
        consume_command(&fixture.database.connection, invocation.turn.id, &snapshot).unwrap();
    assert_eq!(changed.len(), 1);
    let id = format!("command:{}", request.id);
    let consumed = get(&fixture.database.connection, &id)
        .unwrap()
        .unwrap()
        .task;
    assert_eq!(consumed.state, State::Consumed);
    assert!(
        command_finished(&fixture.database.connection, &snapshot, Some(100), false)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        get(&fixture.database.connection, &id)
            .unwrap()
            .unwrap()
            .task,
        consumed
    );
    runs::finish(
        &mut fixture.database,
        invocation.turn.id,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    assert!(
        queue::read(&fixture.database.connection, fixture.session)
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn records_failures_before_process_start() {
    let mut fixture = Fixture::new();
    let queued = fixture.queued();
    for (result, expected) in [
        (
            Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "Fixture start could not be confirmed",
            )),
            Outcome::Unknown,
        ),
        (
            Err(Fault::new(ErrorCode::Busy, "Fixture capacity exhausted")),
            Outcome::Failed,
        ),
        (
            Ok(Output::CommandResult(process::Completion {
                outcome: process::Outcome::Cancelled,
                stdout: Capture::default(),
                stderr: Capture::default(),
                elapsed_ms: 0,
            })),
            Outcome::Cancelled,
        ),
    ] {
        let (request, _) = command(&fixture, queued);
        let returned = records::finish_request(&fixture.database.connection, &request, &result)
            .unwrap()
            .unwrap();
        let record = get(
            &fixture.database.connection,
            &format!("command:{}", request.id),
        )
        .unwrap()
        .unwrap();
        assert_eq!(record.task.state, State::Pending);
        assert_eq!(record.task.outcome, Some(expected));
        assert_eq!(returned, record.task);
    }
}

#[test]
fn recovers_running_commands_without_replay() {
    let mut fixture = Fixture::new();
    let source = fixture.completed(&["Started work"]);
    let (request, _) = command(&fixture, source);
    runs::recover(&fixture.database.connection).unwrap();
    let record = get(
        &fixture.database.connection,
        &format!("command:{}", request.id),
    )
    .unwrap()
    .unwrap();
    assert_eq!(record.task.state, State::Pending);
    assert_eq!(record.task.outcome, Some(Outcome::Interrupted));
    assert!(record.task.recovered && record.task.suppressed);
    assert!(schedule(&fixture).is_none());
}

#[test]
fn failed_admission_preserves_completion() {
    let mut fixture = Fixture::new();
    let source = fixture.completed(&["Started work"]);
    let id = pending(&fixture, source, "Durable result");
    let db = &fixture.database.connection;
    let original = get(db, &id).unwrap().unwrap();
    let mut config = crate::store::commands::session(db, fixture.session)
        .unwrap()
        .config;
    config.credential = Some(CredentialRef {
        node: fixture.database.node,
        id: CredentialId::new(),
    });
    assert_eq!(
        crate::store::commands::validate_config(db, fixture.database.node, &config)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    db.execute(
        "UPDATE session_revisions SET config=?2 WHERE session=?1",
        params![fixture.session.to_string(), encode(&config).unwrap()],
    )
    .unwrap();
    let counts = || {
        ["requests", "turns", "agent_runs", "agent_pending"].map(|table| {
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
        })
    };
    let before = counts();
    let references = fixture.database.feeds.sequence(fixture.session);
    assert!(schedule(&fixture).is_none());
    assert_eq!(counts(), before);
    assert_eq!(fixture.database.feeds.sequence(fixture.session), references);
    assert_eq!(get(db, &id).unwrap().unwrap().task, original.task);
    assert_eq!(get(db, &id).unwrap().unwrap().stdout, original.stdout);
}

#[test]
fn pending_question_blocks_delivery() {
    let mut fixture = Fixture::new();
    let source = fixture.completed(&["Started work"]);
    let id = pending(&fixture, source, "Durable result");
    let entry = fixture.read(None, 20).page.entries[0].id.clone();
    let question = sailry_protocol::conversation::question::Question {
        id: QuestionId::new(),
        session: fixture.session,
        turn: source,
        entry,
        index: 0,
        state: sailry_protocol::conversation::question::State::Pending,
    };
    fixture
        .database
        .connection
        .execute(
            "INSERT INTO agent_questions(id,turn,call,body) VALUES(?1,?2,?3,?4)",
            params![
                question.id.to_string(),
                source.to_string(),
                "fixture-question",
                encode(&question).unwrap()
            ],
        )
        .unwrap();
    assert!(schedule(&fixture).is_none());
    assert_eq!(
        get(&fixture.database.connection, &id)
            .unwrap()
            .unwrap()
            .task
            .delivery,
        None
    );
    fixture
        .database
        .connection
        .execute(
            "DELETE FROM agent_questions WHERE id=?1",
            [question.id.to_string()],
        )
        .unwrap();
    assert!(schedule(&fixture).is_some());
}

#[test]
fn explicit_input_preserves_speech_before_delivery() {
    let mut fixture = Fixture::new();
    let source = fixture.completed(&["Earlier task"]);
    let id = pending(&fixture, source, "Untrusted result text");
    let user = fixture.queued();
    assert!(schedule(&fixture).is_none());
    runs::start(&fixture.database.connection, user).unwrap();
    let invocation = claim(&mut fixture);
    assert!(!invocation.notice);
    assert_eq!(invocation.message.text, "History fixture");
    assert_eq!(invocation.notices.len(), 1);
    assert_eq!(invocation.notices[0].id, id);
    let mut input = event(&invocation);
    input.set_content(
        adk_core::Content::new("user")
            .with_text(invocation.notice_input.as_ref().unwrap())
            .with_text(invocation.message.text.clone()),
    );
    input
        .provider_metadata
        .insert(super::super::sessions::INPUT_SOURCE.into(), "user".into());
    input
        .provider_metadata
        .insert(super::super::sessions::INPUT_PART.into(), "1".into());
    append(&mut fixture, &invocation, input.clone());
    let page = fixture.read(None, 100).page;
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.id == input.id)
        .unwrap();
    assert_eq!(entry.parts, vec![Part::Text("History fixture".into())]);
    assert_eq!(
        get(&fixture.database.connection, &id)
            .unwrap()
            .unwrap()
            .task
            .state,
        State::Delivered
    );
    let canonical: Vec<u8> = fixture
        .database
        .connection
        .query_row(
            "SELECT body FROM agent_events WHERE id=?1",
            [&input.id],
            |row| row.get(0),
        )
        .unwrap();
    let canonical: AdkEvent = serde_json::from_slice(&canonical).unwrap();
    assert_eq!(
        canonical.provider_metadata[super::super::sessions::INPUT_SOURCE],
        "user"
    );
    assert_eq!(
        canonical.provider_metadata[super::super::sessions::INPUT_PART],
        "1"
    );
    assert_eq!(canonical.content().unwrap().parts.len(), 2);
}
