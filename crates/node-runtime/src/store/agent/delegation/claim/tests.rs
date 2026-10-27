use super::*;
use crate::store::agent::tests::{command, fixture::Fixture};

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
    crate::store::plugins::policy::fixture::configure(&fixture.database.connection, value).await;
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
async fn preserves_fifo_when_lowering_budget() {
    let mut fixture = Fixture::new().await;
    let parent = parent(&mut fixture).await;
    limit(&fixture, 2).await;
    let first = queued(&mut fixture, parent, 0).await;
    let second = queued(&mut fixture, parent, 1).await;
    assert_eq!(claim(&mut fixture, second.id).await, Status::Queued);
    assert_eq!(claim(&mut fixture, first.id).await, Status::Running);
    assert_eq!(claim(&mut fixture, second.id).await, Status::Running);
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
    );
    runs::stop(&fixture.database.connection, first.id)
        .await
        .unwrap();
    assert_eq!(claim(&mut fixture, third.id).await, Status::Queued);
    limit(&fixture, 1).await;
    assert_eq!(claim(&mut fixture, third.id).await, Status::Queued);
    finish(&mut fixture, first.id).await;
    assert_eq!(claim(&mut fixture, third.id).await, Status::Queued);
    assert_eq!(
        runs::get(&fixture.database.connection, second.id)
            .await
            .unwrap()
            .status,
        Status::Running
    );
    finish(&mut fixture, second.id).await;
    assert_eq!(claim(&mut fixture, fourth.id).await, Status::Queued);
    assert_eq!(claim(&mut fixture, third.id).await, Status::Running);
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
    );
    assert_eq!(input(&fixture).await, frozen);
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn isolates_session_capacity_and_fifo() {
    let mut fixture = Fixture::new().await;
    let first_parent = parent(&mut fixture).await;
    limit(&fixture, 1).await;
    let first = queued(&mut fixture, first_parent, 0).await;
    let second = queued(&mut fixture, first_parent, 1).await;
    let third = queued(&mut fixture, first_parent, 2).await;
    assert_eq!(claim(&mut fixture, first.id).await, Status::Running);
    assert_eq!(claim(&mut fixture, second.id).await, Status::Queued);

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
        panic!("session expected")
    };
    fixture.session = other.id;
    let other_parent = parent(&mut fixture).await;
    let other_first = queued(&mut fixture, other_parent, 0).await;
    let other_second = queued(&mut fixture, other_parent, 1).await;
    // Both the active count and the oldest eligible turn are session-scoped.
    assert_eq!(claim(&mut fixture, other_second.id).await, Status::Queued);
    assert_eq!(claim(&mut fixture, other_first.id).await, Status::Running);
    assert_eq!(claim(&mut fixture, other_second.id).await, Status::Queued);
    runs::stop(&fixture.database.connection, first.id)
        .await
        .unwrap();
    finish(&mut fixture, other_first.id).await;
    assert_eq!(claim(&mut fixture, other_second.id).await, Status::Running);
    assert_eq!(claim(&mut fixture, second.id).await, Status::Queued);
    finish(&mut fixture, first.id).await;
    assert_eq!(claim(&mut fixture, third.id).await, Status::Queued);
    assert_eq!(claim(&mut fixture, second.id).await, Status::Running);
    assert_eq!(
        runs::get(&fixture.database.connection, other_second.id)
            .await
            .unwrap()
            .status,
        Status::Running
    );
}

#[tokio::test]
async fn cancels_waiters_without_starting_execution() {
    let mut fixture = Fixture::new().await;
    let parent = parent(&mut fixture).await;
    limit(&fixture, 1).await;
    let first = queued(&mut fixture, parent, 0).await;
    let cancelled = queued(&mut fixture, parent, 1).await;
    let next = queued(&mut fixture, parent, 2).await;
    assert_eq!(claim(&mut fixture, first.id).await, Status::Running);
    assert_eq!(claim(&mut fixture, cancelled.id).await, Status::Queued);
    runs::stop(&fixture.database.connection, cancelled.id)
        .await
        .unwrap();
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
        );
    }
    let run = runs::get(&fixture.database.connection, cancelled.id)
        .await
        .unwrap();
    assert!(run.started_ms.is_none());
    assert!(run.finished_ms.is_some());
    let tasks =
        super::super::super::background::list(&fixture.database.connection, fixture.session)
            .await
            .unwrap();
    let task = tasks
        .iter()
        .find(|task| task.id == format!("agent:{}", cancelled.id))
        .unwrap();
    assert_eq!(
        task.outcome,
        Some(sailry_protocol::conversation::background::Outcome::Cancelled)
    );
    finish(&mut fixture, first.id).await;
    assert_eq!(claim(&mut fixture, next.id).await, Status::Running);
}

#[tokio::test]
async fn interrupts_waiters_and_keeps_captures_on_restart() {
    let mut fixture = Fixture::new().await;
    let parent = parent(&mut fixture).await;
    limit(&fixture, 1).await;
    let active = queued(&mut fixture, parent, 0).await;
    let waiting = queued(&mut fixture, parent, 1).await;
    assert_eq!(claim(&mut fixture, active.id).await, Status::Running);
    assert_eq!(claim(&mut fixture, waiting.id).await, Status::Queued);
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
    for turn in [active.id, waiting.id] {
        let run = runs::get(&fixture.database.connection, turn).await.unwrap();
        assert_eq!(run.status, Status::Interrupted);
        assert_eq!(run.error.unwrap().code, ErrorCode::OutcomeUnknown);
        assert_eq!(claim(&mut fixture, turn).await, Status::Interrupted);
    }
    let waiting = runs::hydrate(
        &fixture.database.connection,
        runs::get(&fixture.database.connection, waiting.id)
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    assert!(waiting.started_ms.is_none());
    assert_eq!(waiting.input.unwrap().message.text, "Frozen child 1");
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn keeps_guidance_out_of_unrelated_fifo() {
    let mut fixture = Fixture::new().await;
    let parent = parent(&mut fixture).await;
    limit(&fixture, 1).await;
    let guidance = queued(&mut fixture, parent, 0).await;
    queue::admit(&fixture.database.connection, &guidance, true)
        .await
        .unwrap();
    let other = queued(&mut fixture, parent, 1).await;
    assert_eq!(claim(&mut fixture, other.id).await, Status::Running);
    assert_eq!(claim(&mut fixture, guidance.id).await, Status::Queued);
    assert_eq!(
        runs::get(&fixture.database.connection, guidance.id)
            .await
            .unwrap()
            .status,
        Status::Queued
    );
}

#[tokio::test]
async fn admits_guidance_after_the_recipient_finishes() {
    let mut fixture = Fixture::new().await;
    let parent = parent(&mut fixture).await;
    limit(&fixture, 2).await;
    let recipient = queued(&mut fixture, parent, 0).await;
    assert_eq!(claim(&mut fixture, recipient.id).await, Status::Running);
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
        panic!("guided turn expected")
    };
    assert_eq!(claim(&mut fixture, guidance.id).await, Status::Queued);
    finish(&mut fixture, recipient.id).await;
    let invocation = runs::claim_guidance(&mut fixture.database, recipient.id, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invocation.turn.id, guidance.id);
    assert_eq!(invocation.message.text, "Continue after the recipient");
    assert!(invocation.child.is_some());
    assert_eq!(claim(&mut fixture, guidance.id).await, Status::Running);
    let queue = queue::read(&fixture.database.connection, guidance.session)
        .await
        .unwrap();
    assert!(!queue.paused);
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
    assert_eq!(pending, recipient.id.to_string());
}

#[tokio::test]
async fn applies_increases_without_recapturing_waiters() {
    let mut fixture = Fixture::new().await;
    let parent = parent(&mut fixture).await;
    limit(&fixture, 1).await;
    let first = queued(&mut fixture, parent, 0).await;
    let second = queued(&mut fixture, parent, 1).await;
    // Explicitly selected work can reuse an idle child whose old queue was
    // paused by Stop or restart; unselected guidance remains in agent_pending.
    queue::pause(&fixture.database.connection, second.session, true)
        .await
        .unwrap();
    assert_eq!(claim(&mut fixture, first.id).await, Status::Running);
    assert_eq!(claim(&mut fixture, second.id).await, Status::Queued);
    let capture = async |fixture: &Fixture| {
        fixture.database.connection.prepare("SELECT t.revision,t.plugins,t.roles,a.provider,a.hooks,a.input FROM turns t JOIN agent_runs a ON a.turn=t.id WHERE t.id=?1").await.unwrap().query_row([second.id.to_string()]).await.and_then(|row| Ok((row.get::<i64>(0)?, row.get::<Vec<u8>>(1)?, row.get::<Vec<u8>>(2)?, row.get::<Option<Vec<u8>>>(3)?, row.get::<Vec<u8>>(4)?, row.get::<Vec<u8>>(5)?))).unwrap()
    };
    let frozen = capture(&fixture).await;
    limit(&fixture, u64::MAX).await;
    assert_eq!(claim(&mut fixture, second.id).await, Status::Running);
    assert_eq!(capture(&fixture).await, frozen);
    assert_eq!(
        super::claim(&mut fixture.database, parent, &fixture.events)
            .await
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
}

#[tokio::test]
async fn validates_settings_without_an_extra_ceiling() {
    let fixture = Fixture::new().await;
    let db = &fixture.database.connection;
    assert_eq!(
        crate::store::plugins::policy::child_limit(db)
            .await
            .unwrap(),
        None
    );
    let original = crate::store::plugins::policy::fixture::install(db, "example").await;
    assert_eq!(
        crate::store::plugins::policy::child_limit(db)
            .await
            .unwrap(),
        Some(8)
    );
    let mut state = crate::store::plugins::settings::read(db, &original.summary.reference())
        .await
        .unwrap();
    state
        .values
        .insert("parallel".into(), serde_json::json!("0"));
    assert_eq!(
        crate::store::plugins::settings::save(
            db,
            &state.package,
            &state.values,
            &Default::default()
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
    limit(&fixture, u64::MAX).await;
    assert_eq!(
        crate::store::plugins::policy::child_limit(db)
            .await
            .unwrap(),
        Some(u64::MAX)
    );
    assert_eq!(
        crate::store::plugins::settings::save(
            db,
            &state.package,
            &state.values,
            &Default::default()
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
}
