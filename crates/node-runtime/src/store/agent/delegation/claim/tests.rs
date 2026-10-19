use super::*;
use crate::store::agent::tests::{command, fixture::Fixture};

fn parent(fixture: &mut Fixture) -> TurnId {
    let turn = fixture.queued();
    runs::start(&fixture.database.connection, turn).unwrap();
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .unwrap()
            .turn
            .id,
        turn
    );
    turn
}

fn queued(fixture: &mut Fixture, parent: TurnId, index: usize) -> QueuedTurn {
    let session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    let Output::Session(child) = command(
        &mut fixture.database,
        &fixture.events,
        Command::CreateSession {
            project: session.project,
            worktree: Some(session.worktree),
            config: Some(session.config),
        },
    ) else {
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
    ) else {
        panic!("queued turn expected")
    };
    let db = &fixture.database.connection;
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
    .unwrap();
    // These fixtures model selected, supervised invocations, independently of
    // any plugin package. Unselected guidance keeps its agent_pending row.
    runs::start(db, turn.id).unwrap();
    queue::remove(db, turn.id, turn.session).unwrap();
    runs::capture(db, &turn, false, &message).unwrap();
    let parent = runs::get(db, parent).unwrap();
    let child = runs::get(db, turn.id).unwrap();
    super::super::super::background::child_started(db, &parent, &child, "Neutral child").unwrap();
    turn
}

fn limit(fixture: &Fixture, value: u64) {
    crate::store::plugins::policy::fixture::configure(&fixture.database.connection, value);
}

fn claim(fixture: &mut Fixture, turn: TurnId) -> Status {
    super::claim(&mut fixture.database, turn, &fixture.events).unwrap()
}

fn finish(fixture: &mut Fixture, turn: TurnId) {
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
}

#[test]
fn preserves_fifo_when_lowering_budget() {
    let mut fixture = Fixture::new();
    let parent = parent(&mut fixture);
    limit(&fixture, 2);
    let first = queued(&mut fixture, parent, 0);
    let second = queued(&mut fixture, parent, 1);
    assert_eq!(claim(&mut fixture, second.id), Status::Queued);
    assert_eq!(claim(&mut fixture, first.id), Status::Running);
    assert_eq!(claim(&mut fixture, second.id), Status::Running);
    finish(&mut fixture, parent);
    let next = self::parent(&mut fixture);
    let third = queued(&mut fixture, next, 2);
    let fourth = queued(&mut fixture, next, 3);
    let captured = runs::hydrate(
        &fixture.database.connection,
        runs::get(&fixture.database.connection, third.id).unwrap(),
    )
    .unwrap()
    .input;
    let input = |fixture: &Fixture| {
        fixture
            .database
            .connection
            .query_row(
                "SELECT input FROM agent_runs WHERE turn=?1",
                [third.id.to_string()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .unwrap()
    };
    let frozen = input(&fixture);
    assert_eq!(
        captured,
        Some(sailry_protocol::conversation::SentInput {
            message: serde_json::from_slice(&frozen).unwrap(),
            attachments: vec![],
        })
    );
    runs::stop(&fixture.database.connection, first.id).unwrap();
    assert_eq!(claim(&mut fixture, third.id), Status::Queued);
    limit(&fixture, 1);
    assert_eq!(claim(&mut fixture, third.id), Status::Queued);
    finish(&mut fixture, first.id);
    assert_eq!(claim(&mut fixture, third.id), Status::Queued);
    assert_eq!(
        runs::get(&fixture.database.connection, second.id)
            .unwrap()
            .status,
        Status::Running
    );
    finish(&mut fixture, second.id);
    assert_eq!(claim(&mut fixture, fourth.id), Status::Queued);
    assert_eq!(claim(&mut fixture, third.id), Status::Running);
    assert_eq!(
        runs::hydrate(
            &fixture.database.connection,
            runs::get(&fixture.database.connection, third.id).unwrap()
        )
        .unwrap()
        .input,
        Some(sailry_protocol::conversation::SentInput {
            message: serde_json::from_slice(&frozen).unwrap(),
            attachments: vec![],
        })
    );
    assert_eq!(input(&fixture), frozen);
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
}

#[test]
fn isolates_session_capacity_and_fifo() {
    let mut fixture = Fixture::new();
    let first_parent = parent(&mut fixture);
    limit(&fixture, 1);
    let first = queued(&mut fixture, first_parent, 0);
    let second = queued(&mut fixture, first_parent, 1);
    let third = queued(&mut fixture, first_parent, 2);
    assert_eq!(claim(&mut fixture, first.id), Status::Running);
    assert_eq!(claim(&mut fixture, second.id), Status::Queued);

    let original =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    let Output::Session(other) = command(
        &mut fixture.database,
        &fixture.events,
        Command::CreateSession {
            project: original.project,
            worktree: Some(original.worktree),
            config: Some(original.config),
        },
    ) else {
        panic!("session expected")
    };
    fixture.session = other.id;
    let other_parent = parent(&mut fixture);
    let other_first = queued(&mut fixture, other_parent, 0);
    let other_second = queued(&mut fixture, other_parent, 1);
    // Both the active count and the oldest eligible turn are session-scoped.
    assert_eq!(claim(&mut fixture, other_second.id), Status::Queued);
    assert_eq!(claim(&mut fixture, other_first.id), Status::Running);
    assert_eq!(claim(&mut fixture, other_second.id), Status::Queued);
    runs::stop(&fixture.database.connection, first.id).unwrap();
    finish(&mut fixture, other_first.id);
    assert_eq!(claim(&mut fixture, other_second.id), Status::Running);
    assert_eq!(claim(&mut fixture, second.id), Status::Queued);
    finish(&mut fixture, first.id);
    assert_eq!(claim(&mut fixture, third.id), Status::Queued);
    assert_eq!(claim(&mut fixture, second.id), Status::Running);
    assert_eq!(
        runs::get(&fixture.database.connection, other_second.id)
            .unwrap()
            .status,
        Status::Running
    );
}

#[test]
fn cancels_waiters_without_starting_execution() {
    let mut fixture = Fixture::new();
    let parent = parent(&mut fixture);
    limit(&fixture, 1);
    let first = queued(&mut fixture, parent, 0);
    let cancelled = queued(&mut fixture, parent, 1);
    let next = queued(&mut fixture, parent, 2);
    assert_eq!(claim(&mut fixture, first.id), Status::Running);
    assert_eq!(claim(&mut fixture, cancelled.id), Status::Queued);
    runs::stop(&fixture.database.connection, cancelled.id).unwrap();
    for _ in 0..2 {
        assert_eq!(
            finish_queued(
                &mut fixture.database,
                cancelled.id,
                Status::Cancelled,
                None,
                &fixture.events
            )
            .unwrap(),
            Status::Cancelled
        );
    }
    let run = runs::get(&fixture.database.connection, cancelled.id).unwrap();
    assert!(run.started_ms.is_none());
    assert!(run.finished_ms.is_some());
    let tasks =
        super::super::super::background::list(&fixture.database.connection, fixture.session)
            .unwrap();
    let task = tasks
        .iter()
        .find(|task| task.id == format!("agent:{}", cancelled.id))
        .unwrap();
    assert_eq!(
        task.outcome,
        Some(sailry_protocol::conversation::background::Outcome::Cancelled)
    );
    finish(&mut fixture, first.id);
    assert_eq!(claim(&mut fixture, next.id), Status::Running);
}

#[test]
fn interrupts_waiters_and_keeps_captures_on_restart() {
    let mut fixture = Fixture::new();
    let parent = parent(&mut fixture);
    limit(&fixture, 1);
    let active = queued(&mut fixture, parent, 0);
    let waiting = queued(&mut fixture, parent, 1);
    assert_eq!(claim(&mut fixture, active.id), Status::Running);
    assert_eq!(claim(&mut fixture, waiting.id), Status::Queued);
    runs::recover(&fixture.database.connection).unwrap();
    for turn in [active.id, waiting.id] {
        let run = runs::get(&fixture.database.connection, turn).unwrap();
        assert_eq!(run.status, Status::Interrupted);
        assert_eq!(run.error.unwrap().code, ErrorCode::OutcomeUnknown);
        assert_eq!(claim(&mut fixture, turn), Status::Interrupted);
    }
    let waiting = runs::hydrate(
        &fixture.database.connection,
        runs::get(&fixture.database.connection, waiting.id).unwrap(),
    )
    .unwrap();
    assert!(waiting.started_ms.is_none());
    assert_eq!(waiting.input.unwrap().message.text, "Frozen child 1");
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
}

#[test]
fn keeps_guidance_out_of_unrelated_fifo() {
    let mut fixture = Fixture::new();
    let parent = parent(&mut fixture);
    limit(&fixture, 1);
    let guidance = queued(&mut fixture, parent, 0);
    queue::admit(&fixture.database.connection, &guidance, true).unwrap();
    let other = queued(&mut fixture, parent, 1);
    assert_eq!(claim(&mut fixture, other.id), Status::Running);
    assert_eq!(claim(&mut fixture, guidance.id), Status::Queued);
    assert_eq!(
        runs::get(&fixture.database.connection, guidance.id)
            .unwrap()
            .status,
        Status::Queued
    );
}

#[test]
fn applies_increases_without_recapturing_waiters() {
    let mut fixture = Fixture::new();
    let parent = parent(&mut fixture);
    limit(&fixture, 1);
    let first = queued(&mut fixture, parent, 0);
    let second = queued(&mut fixture, parent, 1);
    // Explicitly selected work can reuse an idle child whose old queue was
    // paused by Stop or restart; unselected guidance remains in agent_pending.
    queue::pause(&fixture.database.connection, second.session, true).unwrap();
    assert_eq!(claim(&mut fixture, first.id), Status::Running);
    assert_eq!(claim(&mut fixture, second.id), Status::Queued);
    let capture = |fixture: &Fixture| {
        fixture.database.connection.query_row(
        "SELECT t.revision,t.plugins,t.roles,a.provider,a.hooks,a.input FROM turns t JOIN agent_runs a ON a.turn=t.id WHERE t.id=?1",
        [second.id.to_string()], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, Vec<u8>>(2)?, row.get::<_, Option<Vec<u8>>>(3)?, row.get::<_, Vec<u8>>(4)?, row.get::<_, Vec<u8>>(5)?)),
    ).unwrap()
    };
    let frozen = capture(&fixture);
    limit(&fixture, u64::MAX);
    assert_eq!(claim(&mut fixture, second.id), Status::Running);
    assert_eq!(capture(&fixture), frozen);
    assert_eq!(
        super::claim(&mut fixture.database, parent, &fixture.events)
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
}

#[test]
fn validates_settings_without_an_extra_ceiling() {
    let fixture = Fixture::new();
    let db = &fixture.database.connection;
    assert_eq!(
        crate::store::plugins::policy::child_limit(db).unwrap(),
        None
    );
    let original = crate::store::plugins::policy::fixture::install(db, "example");
    assert_eq!(
        crate::store::plugins::policy::child_limit(db).unwrap(),
        Some(8)
    );
    let mut state =
        crate::store::plugins::settings::read(db, &original.summary.reference()).unwrap();
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
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
    limit(&fixture, u64::MAX);
    assert_eq!(
        crate::store::plugins::policy::child_limit(db).unwrap(),
        Some(u64::MAX)
    );
    assert_eq!(
        crate::store::plugins::settings::save(
            db,
            &state.package,
            &state.values,
            &Default::default()
        )
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
}
