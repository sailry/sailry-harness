use super::*;
use crate::store::agent::{
    background,
    tests::{command, fixture::Fixture},
};
use sailry_protocol::conversation::background::{Kind, Outcome, State};

async fn parent(fixture: &mut Fixture) -> TurnId {
    let turn = fixture.queued().await;
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap()
            .turn
            .id,
        turn
    );
    turn
}

async fn queued(fixture: &mut Fixture, parent: TurnId, index: usize) -> QueuedTurn {
    let session = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let Output::Session(child) = command(
        &mut fixture.database,
        &fixture.events,
        Command::CreateSession {
            project: session.project,
            worktree: Some(session.worktree),
            config: Some(session.config),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    let message: Input = format!("Frozen child {index}").into();
    let Output::QueuedTurn(turn) = command(
        &mut fixture.database,
        &fixture.events,
        Command::QueueTurn {
            session: child.id,
            expected_revision: child.revision,
            message: message.clone(),
        },
    )
    .await
    else {
        panic!("queued turn expected")
    };
    let db = &fixture.database.connection;
    let transaction = turso::transaction::Transaction::new_unchecked(
        db,
        turso::transaction::TransactionBehavior::Concurrent,
    )
    .await
    .unwrap();
    insert(
        db,
        turn.id,
        &Delegation {
            session: fixture.session,
            turn: parent,
            entry: format!("neutral-call-{index}"),
            index,
            role: None,
        },
    )
    .await
    .unwrap();
    // These fixtures model selected, supervised invocations, independently of
    // any plugin package. Unselected guidance keeps its agent_pending row.
    runs::start(db, turn.id).await.unwrap();
    queue::remove(db, turn.id, turn.session).await.unwrap();
    runs::capture(db, &turn, false, &message).await.unwrap();
    let parent = runs::get(db, parent).await.unwrap();
    let child = runs::get(db, turn.id).await.unwrap();
    super::super::super::background::child_started(db, &parent, &child, "Neutral child")
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    turn
}

async fn limit(fixture: &Fixture, value: u64) {
    let db = &fixture.database.connection;
    let mut settings = crate::store::teams::settings::read(db).await.unwrap();
    settings.child_concurrency = value;
    crate::store::teams::settings::save(db, &settings)
        .await
        .unwrap();
}

async fn claim(fixture: &mut Fixture, turn: TurnId) -> Status {
    super::claim(&mut fixture.database, turn, &fixture.events)
        .await
        .unwrap()
}

async fn finish(fixture: &mut Fixture, turn: TurnId) {
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn preserves_capacity_and_fifo() {
    let mut fixture = Fixture::new().await;
    for (index, scenario) in [
        "preserves_fifo_when_lowering_budget",
        "isolates_session_capacity_and_fifo",
        "cancels_waiters_without_starting_execution",
        "interrupts_waiters_and_keeps_captures_on_restart",
        "keeps_guidance_out_of_unrelated_fifo",
        "admits_guidance_after_the_recipient_finishes",
        "applies_increases_without_recapturing_waiters",
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            fixture.fresh_session().await;
        }
        eprintln!("scenario: {scenario}");
        match scenario {
            "preserves_fifo_when_lowering_budget" => Box::pin(async {
                let parent = parent(&mut fixture).await;
                limit(&fixture, 2).await;
                let first = queued(&mut fixture, parent, 0).await;
                let second = queued(&mut fixture, parent, 1).await;
                assert_eq!(claim(&mut fixture, second.id).await, Status::Queued, "{scenario}");
                assert_eq!(claim(&mut fixture, first.id).await, Status::Running, "{scenario}");
                assert_eq!(claim(&mut fixture, second.id).await, Status::Running, "{scenario}");
                finish(&mut fixture, parent).await;
                let next = self::parent(&mut fixture).await;
                let third = queued(&mut fixture, next, 2).await;
                let fourth = queued(&mut fixture, next, 3).await;
                let captured = runs::hydrate(
                    &fixture.database.connection,
                    runs::get(&fixture.database.connection, third.id)
                        .await
                        .unwrap(),
                )
                .await
                .unwrap()
                .input;
                let input = async |fixture: &Fixture| {
                    fixture
                        .database
                        .connection
                        .prepare("SELECT input FROM agent_runs WHERE turn=?1")
                        .await
                        .unwrap()
                        .query_row([third.id.to_string()])
                        .await
                        .and_then(|row| row.get::<Vec<u8>>(0))
                        .unwrap()
                };
                let frozen = input(&fixture).await;
                assert_eq!(
                    captured,
                    Some(sailry_protocol::conversation::SentInput {
                        message: serde_json::from_slice(&frozen).unwrap(),
                        attachments: vec![],
                    })
                , "{scenario}");
                runs::stop(&fixture.database.connection, first.id)
                    .await
                    .unwrap();
                assert_eq!(claim(&mut fixture, third.id).await, Status::Queued, "{scenario}");
                limit(&fixture, 1).await;
                assert_eq!(claim(&mut fixture, third.id).await, Status::Queued, "{scenario}");
                finish(&mut fixture, first.id).await;
                assert_eq!(claim(&mut fixture, third.id).await, Status::Queued, "{scenario}");
                assert_eq!(
                    runs::get(&fixture.database.connection, second.id)
                        .await
                        .unwrap()
                        .status,
                    Status::Running
                , "{scenario}");
                finish(&mut fixture, second.id).await;
                assert_eq!(claim(&mut fixture, fourth.id).await, Status::Queued, "{scenario}");
                assert_eq!(claim(&mut fixture, third.id).await, Status::Running, "{scenario}");
                assert_eq!(
                    runs::hydrate(
                        &fixture.database.connection,
                        runs::get(&fixture.database.connection, third.id)
                            .await
                            .unwrap()
                    )
                    .await
                    .unwrap()
                    .input,
                    Some(sailry_protocol::conversation::SentInput {
                        message: serde_json::from_slice(&frozen).unwrap(),
                        attachments: vec![],
                    })
                , "{scenario}");
                assert_eq!(input(&fixture).await, frozen, "{scenario}");
                assert!(
                    runs::claim(&mut fixture.database, &fixture.events)
                        .await
                        .unwrap()
                        .is_none()
                , "{scenario}");
            }).await,
            "isolates_session_capacity_and_fifo" => Box::pin(async {
                let first_parent = parent(&mut fixture).await;
                limit(&fixture, 1).await;
                let first = queued(&mut fixture, first_parent, 0).await;
                let second = queued(&mut fixture, first_parent, 1).await;
                let third = queued(&mut fixture, first_parent, 2).await;
                assert_eq!(claim(&mut fixture, first.id).await, Status::Running, "{scenario}");
                assert_eq!(claim(&mut fixture, second.id).await, Status::Queued, "{scenario}");

                let original = crate::store::commands::session(&fixture.database.connection, fixture.session)
                    .await
                    .unwrap();
                let Output::Session(other) = command(
                    &mut fixture.database,
                    &fixture.events,
                    Command::CreateSession {
                        project: original.project,
                        worktree: Some(original.worktree),
                        config: Some(original.config),
                    },
                )
                .await
                else {
                    panic!("{scenario}: session expected")
                };
                fixture.session = other.id;
                let other_parent = parent(&mut fixture).await;
                let other_first = queued(&mut fixture, other_parent, 0).await;
                let other_second = queued(&mut fixture, other_parent, 1).await;
    // Both the active count and the oldest eligible turn are session-scoped.
                assert_eq!(claim(&mut fixture, other_second.id).await, Status::Queued, "{scenario}");
                assert_eq!(claim(&mut fixture, other_first.id).await, Status::Running, "{scenario}");
                assert_eq!(claim(&mut fixture, other_second.id).await, Status::Queued, "{scenario}");
                runs::stop(&fixture.database.connection, first.id)
                    .await
                    .unwrap();
                finish(&mut fixture, other_first.id).await;
                assert_eq!(claim(&mut fixture, other_second.id).await, Status::Running, "{scenario}");
                assert_eq!(claim(&mut fixture, second.id).await, Status::Queued, "{scenario}");
                finish(&mut fixture, first.id).await;
                assert_eq!(claim(&mut fixture, third.id).await, Status::Queued, "{scenario}");
                assert_eq!(claim(&mut fixture, second.id).await, Status::Running, "{scenario}");
                assert_eq!(
                    runs::get(&fixture.database.connection, other_second.id)
                        .await
                        .unwrap()
                        .status,
                    Status::Running
                , "{scenario}");
            }).await,
            "cancels_waiters_without_starting_execution" => Box::pin(async {
                let parent = parent(&mut fixture).await;
                limit(&fixture, 1).await;
                let first = queued(&mut fixture, parent, 0).await;
                let cancelled = queued(&mut fixture, parent, 1).await;
                let next = queued(&mut fixture, parent, 2).await;
                assert_eq!(claim(&mut fixture, first.id).await, Status::Running, "{scenario}");
                assert_eq!(claim(&mut fixture, cancelled.id).await, Status::Queued, "{scenario}");
                let stopped = runs::stop(&fixture.database.connection, cancelled.id)
                    .await
                    .unwrap();
                assert_eq!(stopped.status, Status::Cancelled, "{scenario}");
                assert!(stopped.started_ms.is_none(), "{scenario}");
                assert!(stopped.finished_ms.is_none(), "{scenario}");
                assert_eq!(claim(&mut fixture, cancelled.id).await, Status::Cancelled, "{scenario}");
                let mut settled = None;
                for _ in 0..2 {
                    assert_eq!(
                        finish_queued(
                            &mut fixture.database,
                            cancelled.id,
                            Status::Cancelled,
                            None,
                            &fixture.events
                        )
                        .await
                        .unwrap(),
                        Status::Cancelled
                    , "{scenario}");
                    let run = runs::get(&fixture.database.connection, cancelled.id)
                        .await
                        .unwrap();
                    assert!(run.started_ms.is_none(), "{scenario}");
                    assert!(run.finished_ms.is_some(), "{scenario}");
                    let tasks = background::list(&fixture.database.connection, fixture.session)
                        .await
                        .unwrap();
                    let task = tasks
                        .iter()
                        .find(|task| task.id == format!("agent:{}", cancelled.id))
                        .unwrap();
                    assert_eq!(task.outcome, Some(Outcome::Cancelled), "{scenario}");
                    assert_eq!(task.state, State::Pending, "{scenario}");
                    assert!(task.finished_ms.is_some(), "{scenario}");
                    assert!(task.delivery.is_none(), "{scenario}");
                    let current = (run.finished_ms, task.clone());
                    if let Some(previous) = &settled {
                        assert_eq!(&current, previous, "{scenario}");
                    }
                    settled = Some(current);
                }
                finish(&mut fixture, first.id).await;
                assert_eq!(claim(&mut fixture, next.id).await, Status::Running, "{scenario}");
            }).await,
            "interrupts_waiters_and_keeps_captures_on_restart" => Box::pin(async {
                let parent = parent(&mut fixture).await;
                limit(&fixture, 1).await;
                let active = queued(&mut fixture, parent, 0).await;
                let waiting = queued(&mut fixture, parent, 1).await;
                let cancelled = queued(&mut fixture, parent, 2).await;
                let settled = queued(&mut fixture, parent, 3).await;
                assert_eq!(claim(&mut fixture, active.id).await, Status::Running, "{scenario}");
                assert_eq!(claim(&mut fixture, waiting.id).await, Status::Queued, "{scenario}");
                for turn in [cancelled.id, settled.id] {
                    let run = runs::stop(&fixture.database.connection, turn).await.unwrap();
                    assert!(run.finished_ms.is_none(), "{scenario}");
                }
                assert_eq!(
                    finish_queued(&mut fixture.database, settled.id, Status::Cancelled, None, &fixture.events)
                        .await
                        .unwrap(),
                    Status::Cancelled,
                    "{scenario}"
                );
                let finished = runs::get(&fixture.database.connection, settled.id).await.unwrap().finished_ms;
                assert!(finished.is_some(), "{scenario}");
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
                for turn in [active.id, waiting.id, cancelled.id] {
                    let run = runs::get(&fixture.database.connection, turn).await.unwrap();
                    assert_eq!(run.status, Status::Interrupted, "{scenario}");
                    assert_eq!(run.error.unwrap().code, ErrorCode::OutcomeUnknown, "{scenario}");
                    assert_eq!(claim(&mut fixture, turn).await, Status::Interrupted, "{scenario}");
                }
                let run = runs::get(&fixture.database.connection, settled.id).await.unwrap();
                assert_eq!(run.status, Status::Cancelled, "{scenario}");
                assert_eq!(run.finished_ms, finished, "{scenario}");
                let cancelled = runs::hydrate(
                    &fixture.database.connection,
                    runs::get(&fixture.database.connection, cancelled.id).await.unwrap(),
                ).await.unwrap();
                assert!(cancelled.started_ms.is_none(), "{scenario}");
                assert!(cancelled.finished_ms.is_some(), "{scenario}");
                assert_eq!(cancelled.input.unwrap().message.text, "Frozen child 2", "{scenario}");
                let tasks = background::list(&fixture.database.connection, fixture.session).await.unwrap();
                for (turn, outcome) in [
                    (active.id, Outcome::Interrupted),
                    (waiting.id, Outcome::Interrupted),
                    (cancelled.turn, Outcome::Interrupted),
                    (settled.id, Outcome::Cancelled),
                ] {
                    let task = tasks.iter().find(|task| task.id == format!("agent:{turn}")).unwrap();
                    assert_eq!(task.outcome, Some(outcome), "{scenario}");
                    assert!(task.recovered, "{scenario}");
                    assert!(task.suppressed, "{scenario}");
                }
                let waiting = runs::hydrate(
                    &fixture.database.connection,
                    runs::get(&fixture.database.connection, waiting.id)
                        .await
                        .unwrap(),
                )
                .await
                .unwrap();
                assert!(waiting.started_ms.is_none(), "{scenario}");
                assert_eq!(waiting.input.unwrap().message.text, "Frozen child 1", "{scenario}");
                assert!(
                    runs::claim(&mut fixture.database, &fixture.events)
                        .await
                        .unwrap()
                        .is_none()
                , "{scenario}");
            }).await,
            "keeps_guidance_out_of_unrelated_fifo" => Box::pin(async {
                let parent = parent(&mut fixture).await;
                limit(&fixture, 1).await;
                let guidance = queued(&mut fixture, parent, 0).await;
                queue::admit(&fixture.database.connection, &guidance, true)
                    .await
                    .unwrap();
                let other = queued(&mut fixture, parent, 1).await;
                assert_eq!(claim(&mut fixture, other.id).await, Status::Running, "{scenario}");
                assert_eq!(claim(&mut fixture, guidance.id).await, Status::Queued, "{scenario}");
                assert_eq!(
                    runs::get(&fixture.database.connection, guidance.id)
                        .await
                        .unwrap()
                        .status,
                    Status::Queued
                , "{scenario}");
                let stopped = runs::stop(&fixture.database.connection, guidance.id).await.unwrap();
                assert_eq!(stopped.status, Status::Cancelled, "{scenario}");
                assert!(stopped.started_ms.is_none(), "{scenario}");
                assert!(stopped.finished_ms.is_some(), "{scenario}");
            }).await,
            "admits_guidance_after_the_recipient_finishes" => Box::pin(async {
                let parent = parent(&mut fixture).await;
                limit(&fixture, 2).await;
                let recipient = queued(&mut fixture, parent, 0).await;
                assert_eq!(claim(&mut fixture, recipient.id).await, Status::Running, "{scenario}");
                let Output::QueuedTurn(guidance) = command(
                    &mut fixture.database,
                    &fixture.events,
                    Command::GuideTurn {
                        recipient: recipient.id,
                        expected_revision: recipient.revision,
                        message: "Continue after the recipient".into(),
                    },
                )
                .await
                else {
                    panic!("{scenario}: guided turn expected")
                };
                assert_eq!(claim(&mut fixture, guidance.id).await, Status::Queued, "{scenario}");
                finish(&mut fixture, recipient.id).await;
                let invocation = runs::claim_guidance(&mut fixture.database, recipient.id, &fixture.events)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(invocation.turn.id, guidance.id, "{scenario}");
                assert_eq!(invocation.message.text, "Continue after the recipient", "{scenario}");
                assert!(invocation.child.is_some(), "{scenario}");
                assert_eq!(claim(&mut fixture, guidance.id).await, Status::Running, "{scenario}");
                let queue = queue::read(&fixture.database.connection, guidance.session)
                    .await
                    .unwrap();
                assert!(!queue.paused, "{scenario}");
                let pending = fixture
                    .database
                    .connection
                    .prepare("SELECT recipient FROM agent_pending WHERE turn=?1")
                    .await
                    .unwrap()
                    .query_row([guidance.id.to_string()])
                    .await
                    .unwrap()
                    .get::<String>(0)
                    .unwrap();
                assert_eq!(pending, recipient.id.to_string(), "{scenario}");
            }).await,
            "applies_increases_without_recapturing_waiters" => Box::pin(async {
                let parent = parent(&mut fixture).await;
                limit(&fixture, 1).await;
                let first = queued(&mut fixture, parent, 0).await;
                let second = queued(&mut fixture, parent, 1).await;
    // Explicitly selected work can reuse an idle child whose old queue was
    // paused by Stop or restart; unselected guidance remains in agent_pending.
                queue::pause(&fixture.database.connection, second.session, true)
                    .await
                    .unwrap();
                assert_eq!(claim(&mut fixture, first.id).await, Status::Running, "{scenario}");
                assert_eq!(claim(&mut fixture, second.id).await, Status::Queued, "{scenario}");
                let capture = async |fixture: &Fixture| {
                    fixture.database.connection.prepare("SELECT t.revision,t.plugins,t.roles,a.provider,a.hooks,a.input FROM turns t JOIN agent_runs a ON a.turn=t.id WHERE t.id=?1").await.unwrap().query_row([second.id.to_string()]).await.and_then(|row| Ok((row.get::<i64>(0)?, row.get::<Vec<u8>>(1)?, row.get::<Vec<u8>>(2)?, row.get::<Option<Vec<u8>>>(3)?, row.get::<Vec<u8>>(4)?, row.get::<Vec<u8>>(5)?))).unwrap()
                };
                let frozen = capture(&fixture).await;
                limit(&fixture, u64::MAX).await;
                assert_eq!(claim(&mut fixture, second.id).await, Status::Running, "{scenario}");
                assert_eq!(capture(&fixture).await, frozen, "{scenario}");
                assert_eq!(
                    super::claim(&mut fixture.database, parent, &fixture.events)
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::WrongTarget
                , "{scenario}");
            }).await,
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
async fn settles_selected_guidance_only() {
    let mut fixture = Fixture::new().await;
    let parent = parent(&mut fixture).await;
    let ordinary = fixture.queued().await;
    let stopped = runs::stop(&fixture.database.connection, ordinary)
        .await
        .unwrap();
    assert_eq!(stopped.status, Status::Cancelled);
    assert!(stopped.started_ms.is_none());
    assert!(stopped.finished_ms.is_some());

    let child = queued(&mut fixture, parent, 0).await;
    assert_eq!(claim(&mut fixture, child.id).await, Status::Running);
    let Output::QueuedTurn(withdrawn) = command(
        &mut fixture.database,
        &fixture.events,
        Command::GuideTurn {
            recipient: child.id,
            expected_revision: child.revision,
            message: "Discard this guidance".into(),
        },
    )
    .await
    else {
        panic!("guided turn expected")
    };
    let before = background::list(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].state, State::Running);
    assert!(before[0].outcome.is_none());
    let stopped = runs::stop(&fixture.database.connection, withdrawn.id)
        .await
        .unwrap();
    assert_eq!(stopped.status, Status::Cancelled);
    assert!(stopped.started_ms.is_none());
    assert!(stopped.finished_ms.is_some());
    assert_eq!(
        finish_queued(
            &mut fixture.database,
            withdrawn.id,
            Status::Cancelled,
            None,
            &fixture.events,
        )
        .await
        .unwrap(),
        Status::Cancelled,
    );
    assert_eq!(
        background::list(&fixture.database.connection, fixture.session)
            .await
            .unwrap(),
        before,
    );
    assert_eq!(
        runs::get(&fixture.database.connection, child.id)
            .await
            .unwrap()
            .status,
        Status::Running,
    );

    let Output::QueuedTurn(guidance) = command(
        &mut fixture.database,
        &fixture.events,
        Command::GuideTurn {
            recipient: child.id,
            expected_revision: child.revision,
            message: "Selected guidance".into(),
        },
    )
    .await
    else {
        panic!("guided turn expected")
    };
    finish(&mut fixture, child.id).await;
    let invocation = runs::claim_guidance(&mut fixture.database, child.id, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invocation.turn.id, guidance.id);
    finish(&mut fixture, parent).await;
    let stopped = runs::stop(&fixture.database.connection, guidance.id)
        .await
        .unwrap();
    assert_eq!(stopped.status, Status::Cancelled);
    assert!(stopped.started_ms.is_none());
    assert!(stopped.finished_ms.is_none());
    assert_eq!(
        finish_queued(
            &mut fixture.database,
            guidance.id,
            Status::Cancelled,
            None,
            &fixture.events,
        )
        .await
        .unwrap(),
        Status::Cancelled,
    );
    let tasks = background::list(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(tasks.len(), 1);
    let task = &tasks[0];
    assert_eq!(task.id, format!("agent:{}", child.id));
    assert_eq!(
        task.kind,
        Kind::Agent {
            session: child.session,
            turn: guidance.id,
        },
    );
    assert_eq!(task.state, State::Pending);
    assert_eq!(task.outcome, Some(Outcome::Cancelled));
    assert!(task.delivery.is_none());
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
    let next = self::parent(&mut fixture).await;
    let run = runs::get(&fixture.database.connection, next).await.unwrap();
    assert_eq!(run.notices.len(), 1);
    assert_eq!(run.notices[0].id, task.id);
    assert_eq!(run.notices[0].outcome, Outcome::Cancelled);
    let tasks = background::list(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(tasks[0].delivery, Some(next));
}

#[tokio::test]
async fn reads_the_node_budget() {
    let fixture = Fixture::new().await;
    let db = &fixture.database.connection;
    assert_eq!(
        crate::store::teams::settings::read(db)
            .await
            .unwrap()
            .child_concurrency,
        8
    );
    limit(&fixture, u64::MAX).await;
    let saved = crate::store::teams::settings::read(db).await.unwrap();
    assert_eq!((saved.revision, saved.child_concurrency), (1, u64::MAX));
    assert_eq!(
        crate::store::teams::settings::save(
            db,
            &sailry_protocol::team::Settings {
                revision: 0,
                child_concurrency: 2,
            },
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
}
