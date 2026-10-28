use super::*;
use crate::agent::canonical::{self, BlockKind};
use crate::store::agent::tests::fixture::{self, Fixture};
use sailry_protocol::process::{Access, Capture, Info, Snapshot};
use serde_json::Value;
use std::collections::BTreeSet;

async fn pending(fixture: &Fixture, turn: TurnId, output: &str) -> String {
    let transaction = turso::transaction::Transaction::new_unchecked(
        &fixture.database.connection,
        turso::transaction::TransactionBehavior::Concurrent,
    )
    .await
    .unwrap();
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
        sequence: next_sequence(&fixture.database.connection).await.unwrap(),
    };
    save(&fixture.database.connection, &mut record)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    record.task.id
}

async fn schedule(fixture: &Fixture) -> Option<QueuedTurn> {
    let transaction = turso::transaction::Transaction::new_unchecked(
        &fixture.database.connection,
        turso::transaction::TransactionBehavior::Concurrent,
    )
    .await
    .unwrap();
    let scheduled = super::schedule(
        &fixture.database.connection,
        fixture.database.node,
        fixture.session,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    scheduled
}

async fn claim(fixture: &mut Fixture) -> Invocation {
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap()
}

fn event(invocation: &Invocation) -> canonical::Record {
    let mut record = fixture::message(
        "user",
        canonical::Role::User,
        vec![BlockKind::Text {
            text: invocation.notice_input.clone().unwrap(),
        }],
    );
    record.turn = invocation.turn.id;
    record.metadata.input = Some(canonical::Input {
        turn: Some(invocation.turn.id),
        source: Some(canonical::InputSource::Notice),
        notice_part: Some(0),
        notice_ids: invocation
            .notices
            .iter()
            .map(|notice| notice.id.clone())
            .collect(),
        ..Default::default()
    });
    record
}

async fn append(fixture: &mut Fixture, invocation: &Invocation, event: canonical::Record) {
    super::super::history::append(
        &mut fixture.database,
        fixture.session,
        invocation.turn.id,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn delivers_all_bounded_batches() {
    for output in ["Result".to_owned(), "\n".repeat(2048)] {
        let mut fixture = Fixture::new().await;
        let source = fixture.completed(&["Started work"]).await;
        let mut ids = BTreeSet::new();
        for _ in 0..17 {
            ids.insert(pending(&fixture, source, &output).await);
        }
        let queued = schedule(&fixture).await.unwrap();
        let reserved = runs::get(&fixture.database.connection, queued.id)
            .await
            .unwrap();
        assert!(reserved.notice);
        assert!(!reserved.notices.is_empty());
        let mut delivered = BTreeSet::new();
        let mut batches = 0;
        while !queue::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .items
            .is_empty()
        {
            let invocation = claim(&mut fixture).await;
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
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(record.task.state, State::Pending);
                assert_eq!(record.task.delivery, Some(invocation.turn.id));
                assert!(delivered.insert(notice.id.clone()));
            }
            append(&mut fixture, &invocation, event(&invocation)).await;
            for notice in &invocation.notices {
                assert_eq!(
                    get(&fixture.database.connection, &notice.id)
                        .await
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
            .await
            .unwrap();
            batches += 1;
        }
        assert!(batches >= 2);
        assert_eq!(delivered, ids);
        assert!(schedule(&fixture).await.is_none());
    }
}

#[tokio::test]
async fn cancelling_reservation_prevents_rescheduling() {
    let mut fixture = Fixture::new().await;
    let source = fixture.completed(&["Started work"]).await;
    let id = pending(&fixture, source, "Result").await;
    let queued = schedule(&fixture).await.unwrap();
    assert_eq!(
        get(&fixture.database.connection, &id)
            .await
            .unwrap()
            .unwrap()
            .task
            .delivery,
        Some(queued.id)
    );
    runs::stop(&fixture.database.connection, queued.id)
        .await
        .unwrap();
    let record = get(&fixture.database.connection, &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.task.state, State::Pending);
    assert_eq!(record.task.delivery, None);
    assert!(record.task.suppressed);
    assert!(schedule(&fixture).await.is_none());
    let newer = pending(&fixture, source, "Later result").await;
    let next = schedule(&fixture).await.unwrap();
    let run = runs::get(&fixture.database.connection, next.id)
        .await
        .unwrap();
    assert_eq!(
        run.notices
            .iter()
            .map(|notice| &notice.id)
            .collect::<Vec<_>>(),
        vec![&newer]
    );
    assert_eq!(
        get(&fixture.database.connection, &id)
            .await
            .unwrap()
            .unwrap()
            .task
            .delivery,
        None
    );
}

#[tokio::test]
async fn recovers_only_unconfirmed_delivery() {
    for confirmed in [false, true] {
        let mut fixture = Fixture::new().await;
        let source = fixture.completed(&["Started work"]).await;
        let id = pending(&fixture, source, "Durable result").await;
        schedule(&fixture).await.unwrap();
        let invocation = claim(&mut fixture).await;
        if confirmed {
            append(&mut fixture, &invocation, event(&invocation)).await;
        }
        {
            let transaction = turso::transaction::Transaction::new_unchecked(
                &fixture.database.connection,
                turso::transaction::TransactionBehavior::Concurrent,
            )
            .await
            .unwrap();
            runs::recover(&transaction).await.unwrap();
            transaction.commit().await.unwrap();
        };
        let record = get(&fixture.database.connection, &id)
            .await
            .unwrap()
            .unwrap();
        if confirmed {
            assert_eq!(record.task.state, State::Delivered);
            assert_eq!(record.task.delivery, Some(invocation.turn.id));
        } else {
            assert_eq!(record.task.state, State::Pending);
            assert_eq!(record.task.delivery, None);
            assert!(record.task.recovered && record.task.suppressed);
        }
        assert!(schedule(&fixture).await.is_none());
    }
}

#[tokio::test]
async fn rejects_forged_canonical_confirmation() {
    let mut fixture = Fixture::new().await;
    let source = fixture.completed(&["Started work"]).await;
    let id = pending(&fixture, source, "Durable result").await;
    schedule(&fixture).await.unwrap();
    let invocation = claim(&mut fixture).await;
    let mut forged = event(&invocation);
    forged.message_mut().unwrap().content[0].kind = BlockKind::Text {
        text: "Different evidence".into(),
    };
    assert!(
        super::super::history::append(
            &mut fixture.database,
            fixture.session,
            invocation.turn.id,
            forged.clone(),
            &fixture.events
        )
        .await
        .is_err()
    );
    assert_eq!(
        get(&fixture.database.connection, &id)
            .await
            .unwrap()
            .unwrap()
            .task
            .state,
        State::Pending
    );
    let count: i64 = fixture
        .database
        .connection
        .prepare("SELECT count(*) FROM agent_events WHERE id=?1")
        .await
        .unwrap()
        .query_row([forged.id.as_str()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let canonical = event(&invocation);
    append(&mut fixture, &invocation, canonical.clone()).await;
    let revision = get(&fixture.database.connection, &id)
        .await
        .unwrap()
        .unwrap()
        .task
        .revision;
    append(&mut fixture, &invocation, canonical).await;
    assert_eq!(
        get(&fixture.database.connection, &id)
            .await
            .unwrap()
            .unwrap()
            .task
            .revision,
        revision
    );
}

async fn command(fixture: &Fixture, turn: TurnId) -> (Request, Snapshot) {
    let transaction = turso::transaction::Transaction::new_unchecked(
        &fixture.database.connection,
        turso::transaction::TransactionBehavior::Concurrent,
    )
    .await
    .unwrap();
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
        .await
        .unwrap()
        .unwrap();
    transaction.commit().await.unwrap();
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

#[tokio::test]
async fn terminal_read_wins_late_completion() {
    let mut fixture = Fixture::new().await;
    let queued = fixture.queued().await;
    runs::start(&fixture.database.connection, queued)
        .await
        .unwrap();
    let invocation = claim(&mut fixture).await;
    let (request, snapshot) = command(&fixture, invocation.turn.id).await;
    let changed = {
        let transaction = turso::transaction::Transaction::new_unchecked(
            &fixture.database.connection,
            turso::transaction::TransactionBehavior::Concurrent,
        )
        .await
        .unwrap();
        let changed = consume_command(&transaction, invocation.turn.id, &snapshot)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        changed
    };
    assert_eq!(changed.len(), 1);
    let id = format!("command:{}", request.id);
    let consumed = get(&fixture.database.connection, &id)
        .await
        .unwrap()
        .unwrap()
        .task;
    assert_eq!(consumed.state, State::Consumed);
    assert!(
        command_finished(&fixture.database.connection, &snapshot, Some(100), false)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        get(&fixture.database.connection, &id)
            .await
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
    .await
    .unwrap();
    assert!(
        queue::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn records_failures_before_process_start() {
    let mut fixture = Fixture::new().await;
    let queued = fixture.queued().await;
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
        let (request, _) = command(&fixture, queued).await;
        let returned = {
            let transaction = turso::transaction::Transaction::new_unchecked(
                &fixture.database.connection,
                turso::transaction::TransactionBehavior::Concurrent,
            )
            .await
            .unwrap();
            let returned = records::finish_request(&transaction, &request, &result)
                .await
                .unwrap()
                .unwrap();
            transaction.commit().await.unwrap();
            returned
        };
        let record = get(
            &fixture.database.connection,
            &format!("command:{}", request.id),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(record.task.state, State::Pending);
        assert_eq!(record.task.outcome, Some(expected));
        assert_eq!(returned, record.task);
    }
}

#[tokio::test]
async fn recovers_running_commands_without_replay() {
    let mut fixture = Fixture::new().await;
    let source = fixture.completed(&["Started work"]).await;
    let (request, _) = command(&fixture, source).await;
    {
        let transaction = turso::transaction::Transaction::new_unchecked(
            &fixture.database.connection,
            turso::transaction::TransactionBehavior::Concurrent,
        )
        .await
        .unwrap();
        runs::recover(&transaction).await.unwrap();
        transaction.commit().await.unwrap();
    };
    let record = get(
        &fixture.database.connection,
        &format!("command:{}", request.id),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(record.task.state, State::Pending);
    assert_eq!(record.task.outcome, Some(Outcome::Interrupted));
    assert!(record.task.recovered && record.task.suppressed);
    assert!(schedule(&fixture).await.is_none());
}

#[tokio::test]
async fn failed_admission_preserves_completion() {
    let mut fixture = Fixture::new().await;
    let source = fixture.completed(&["Started work"]).await;
    let id = pending(&fixture, source, "Durable result").await;
    let db = &fixture.database.connection;
    let original = get(db, &id).await.unwrap().unwrap();
    let mut config = crate::store::commands::session(db, fixture.session)
        .await
        .unwrap()
        .config;
    config.credential = Some(CredentialRef {
        node: fixture.database.node,
        id: CredentialId::new(),
    });
    assert_eq!(
        crate::store::commands::validate_config(db, fixture.database.node, &config)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    db.execute(
        "UPDATE session_revisions SET config=?2 WHERE session=?1",
        params![fixture.session.to_string(), encode(&config).unwrap()],
    )
    .await
    .unwrap();
    let counts = async || {
        let mut counts = [0i64; 4];
        for (index, table) in ["requests", "turns", "agent_runs", "agent_pending"]
            .into_iter()
            .enumerate()
        {
            counts[index] = db
                .prepare(format!("SELECT count(*) FROM {table}"))
                .await
                .unwrap()
                .query_row(())
                .await
                .and_then(|row| row.get::<i64>(0))
                .unwrap();
        }
        counts
    };
    let before = counts().await;
    let references = fixture.database.feeds.sequence(fixture.session);
    assert!(schedule(&fixture).await.is_none());
    assert_eq!(counts().await, before);
    assert_eq!(fixture.database.feeds.sequence(fixture.session), references);
    assert_eq!(get(db, &id).await.unwrap().unwrap().task, original.task);
    assert_eq!(get(db, &id).await.unwrap().unwrap().stdout, original.stdout);
}

#[tokio::test]
async fn pending_question_blocks_delivery() {
    let mut fixture = Fixture::new().await;
    let source = fixture.completed(&["Started work"]).await;
    let id = pending(&fixture, source, "Durable result").await;
    let entry = fixture.read(None, 20).await.page.entries[0].id.clone();
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
        .await
        .unwrap();
    assert!(schedule(&fixture).await.is_none());
    assert_eq!(
        get(&fixture.database.connection, &id)
            .await
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
        .await
        .unwrap();
    assert!(schedule(&fixture).await.is_some());
}

#[tokio::test]
async fn explicit_input_preserves_speech_before_delivery() {
    let mut fixture = Fixture::new().await;
    let source = fixture.completed(&["Earlier task"]).await;
    let id = pending(&fixture, source, "Untrusted result text").await;
    let user = fixture.queued().await;
    assert!(schedule(&fixture).await.is_none());
    runs::start(&fixture.database.connection, user)
        .await
        .unwrap();
    let invocation = claim(&mut fixture).await;
    assert!(!invocation.notice);
    assert_eq!(invocation.message.text, "History fixture");
    assert_eq!(invocation.notices.len(), 1);
    assert_eq!(invocation.notices[0].id, id);
    let mut input = event(&invocation);
    let content = input.message_mut().unwrap();
    content.part_count = 2;
    content.content.push(canonical::Block {
        index: 1,
        kind: BlockKind::Text {
            text: invocation.message.text.clone(),
        },
    });
    let metadata = input.metadata.input.as_mut().unwrap();
    metadata.source = Some(canonical::InputSource::User);
    metadata.text_part = Some(1);
    append(&mut fixture, &invocation, input.clone()).await;
    let page = fixture.read(None, 100).await.page;
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.id == input.id)
        .unwrap();
    assert_eq!(entry.parts, vec![Part::Text("History fixture".into())]);
    assert_eq!(
        get(&fixture.database.connection, &id)
            .await
            .unwrap()
            .unwrap()
            .task
            .state,
        State::Delivered
    );
    let canonical: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT body FROM agent_events WHERE id=?1")
        .await
        .unwrap()
        .query_row([input.id.as_str()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    let canonical: crate::agent::canonical::Record = serde_json::from_slice(&canonical).unwrap();
    let input = canonical.metadata.input.as_ref().unwrap();
    assert!(matches!(
        input.source,
        Some(crate::agent::canonical::InputSource::User)
    ));
    assert_eq!(input.text_part, Some(1));
    assert_eq!(input.notice_part, Some(0));
    let message = canonical.message().unwrap();
    assert_eq!(message.part_count, 2);
    assert_eq!(message.content.len(), 2);
    assert_eq!(message.content[0].index, 0);
    assert!(matches!(
        &message.content[0].kind,
        crate::agent::canonical::BlockKind::Text { text }
            if Some(text) == invocation.notice_input.as_ref()
    ));
    assert_eq!(message.content[1].index, 1);
}
