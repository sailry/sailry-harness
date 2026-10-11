use super::*;
use sailry_protocol::plugin::{storage::ConversationEntry, transaction::Operation};
use serde_json::json;

fn value(state: &str) -> serde_json::Value {
    json!({
        "id":"00000000-0000-4000-8000-000000000001",
        "description":"Verify the resumed work",
        "state":state,
        "after":null,
    })
}

fn setup() -> (Fixture, plugin::Context) {
    let mut fixture = Fixture::new();
    let context = prepare(&mut fixture);
    execute(
        &mut fixture,
        &context,
        Command::WritePluginConversationValue {
            key: "goal".into(),
            value: value("paused"),
            expected_revision: 0,
        },
    )
    .unwrap();
    (fixture, context)
}

fn goal(fixture: &Fixture) -> ConversationEntry {
    crate::store::plugins::storage::conversation::read(
        &fixture.database.connection,
        "goals",
        fixture.session,
        "goal",
    )
    .unwrap()
}

fn operations(session: SessionId, queue_revision: u64, goal_revision: u64) -> Vec<Operation> {
    vec![
        Operation::QueuePause {
            session,
            expected_revision: queue_revision,
            paused: false,
        },
        Operation::ConversationWrite {
            key: "goal".into(),
            value: value("active"),
            expected_revision: goal_revision,
        },
        Operation::Continue {
            session,
            after: None,
            key: "goal".into(),
            expected_revision: goal_revision + 1,
            scope: plugin::storage::Scope::Conversation,
            message: "Continue the requested work".into(),
        },
    ]
}

fn admit(fixture: &mut Fixture, context: &plugin::Context) -> TurnId {
    let queue = queue::read(&fixture.database.connection, fixture.session).unwrap();
    let operations = operations(fixture.session, queue.revision, goal(fixture).revision);
    let Output::PluginTransaction(outputs) =
        execute(fixture, context, Command::PluginTransaction { operations }).unwrap()
    else {
        panic!("transaction expected")
    };
    let Output::QueuedTurn(turn) = &outputs[2] else {
        panic!("turn expected")
    };
    turn.id
}

fn rejects(
    fixture: &mut Fixture,
    context: &plugin::Context,
    operations: Vec<Operation>,
    code: ErrorCode,
) {
    let queue = queue::read(&fixture.database.connection, fixture.session).unwrap();
    let state = goal(fixture);
    let runs = runs::list(&fixture.database.connection, fixture.session, &[]).unwrap();
    assert_eq!(
        execute(fixture, context, Command::PluginTransaction { operations })
            .unwrap_err()
            .code,
        code
    );
    assert_eq!(
        queue::read(&fixture.database.connection, fixture.session).unwrap(),
        queue
    );
    assert_eq!(goal(fixture), state);
    assert_eq!(
        runs::list(&fixture.database.connection, fixture.session, &[]).unwrap(),
        runs
    );
}

#[test]
fn stopped_queue_preserves_fence_and_marker() {
    let (mut fixture, context) = setup();
    let original = fixture.queued();
    runs::start(&fixture.database.connection, original).unwrap();
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .unwrap()
            .turn
            .id,
        original
    );
    runs::stop(&fixture.database.connection, original).unwrap();
    runs::finish(
        &mut fixture.database,
        original,
        Status::Cancelled,
        None,
        &fixture.events,
    )
    .unwrap();
    let paused = queue::read(&fixture.database.connection, fixture.session).unwrap();
    assert!(paused.paused);
    assert!(paused.items.is_empty());
    assert_eq!(
        continue_turn(&mut fixture, &context, None)
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );

    let turn = admit(&mut fixture, &context);
    let queue = queue::read(&fixture.database.connection, fixture.session).unwrap();
    assert!(!queue.paused);
    assert_eq!(queue.items.len(), 1);
    assert_eq!(queue.items[0].turn, turn);
    assert!(queue.items[0].ready);
    assert_eq!(goal(&fixture).value["state"], "active");
    assert_eq!(goal(&fixture).revision, 2);
    let run = runs::get(&fixture.database.connection, turn).unwrap();
    let continuation = run.continuation.as_ref().unwrap();
    assert_eq!(continuation.package, context.package);
    assert_eq!(
        continuation.marker.as_ref().unwrap().label("en"),
        "Continue goal"
    );
    assert_eq!(
        continuation.marker.as_ref().unwrap().label("zh-CN"),
        "继续执行目标"
    );
    let invocation = runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    assert_eq!(invocation.turn.id, turn);
    assert!(invocation.automatic);
    assert_eq!(invocation.message.text, "Continue the requested work");
    assert_eq!(
        runs::get(&fixture.database.connection, turn)
            .unwrap()
            .continuation,
        run.continuation
    );
}

#[test]
fn held_input_rolls_back_state_and_queue() {
    let (mut fixture, context) = setup();
    let held = fixture.queued();
    queue::pause(&fixture.database.connection, fixture.session, true).unwrap();
    let queue = queue::read(&fixture.database.connection, fixture.session).unwrap();
    assert_eq!(queue.items[0].turn, held);
    assert!(!queue.items[0].ready);
    let operations = operations(fixture.session, queue.revision, goal(&fixture).revision);
    rejects(&mut fixture, &context, operations, ErrorCode::Busy);
}

#[test]
fn active_work_rolls_back_state_and_queue() {
    for stopping in [false, true] {
        let (mut fixture, context) = setup();
        let active = fixture.queued();
        runs::start(&fixture.database.connection, active).unwrap();
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .unwrap();
        if stopping {
            runs::stop(&fixture.database.connection, active).unwrap();
        } else {
            queue::pause(&fixture.database.connection, fixture.session, true).unwrap();
        }
        let queue = queue::read(&fixture.database.connection, fixture.session).unwrap();
        let operations = operations(fixture.session, queue.revision, goal(&fixture).revision);
        rejects(&mut fixture, &context, operations, ErrorCode::Busy);
    }
}

#[test]
fn stale_revisions_roll_back_state_and_queue() {
    for stale_queue in [false, true] {
        let (mut fixture, context) = setup();
        queue::pause(&fixture.database.connection, fixture.session, true).unwrap();
        let queue = queue::read(&fixture.database.connection, fixture.session).unwrap();
        let operations = operations(
            fixture.session,
            queue.revision - u64::from(stale_queue),
            goal(&fixture).revision - u64::from(!stale_queue),
        );
        rejects(
            &mut fixture,
            &context,
            operations,
            ErrorCode::RevisionConflict,
        );
    }
}

#[test]
fn preserves_session_scope_and_capability_checks() {
    let (mut fixture, context) = setup();
    queue::pause(&fixture.database.connection, fixture.session, true).unwrap();
    let current =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    let Output::Session(other) = command(
        &mut fixture.database,
        &fixture.events,
        Command::CreateSession {
            project: current.project,
            worktree: Some(current.worktree),
            config: Some(current.config),
        },
    ) else {
        panic!("session expected")
    };
    queue::pause(&fixture.database.connection, other.id, true).unwrap();
    let other_queue = queue::read(&fixture.database.connection, other.id).unwrap();
    rejects(
        &mut fixture,
        &context,
        vec![Operation::QueuePause {
            session: other.id,
            expected_revision: other_queue.revision,
            paused: false,
        }],
        ErrorCode::PermissionDenied,
    );
    assert_eq!(
        queue::read(&fixture.database.connection, other.id).unwrap(),
        other_queue
    );

    fixture::package(
        &fixture.database,
        &fixture._root.path().join("profile"),
        "reminders",
    );
    let mut unauthorized = context.clone();
    unauthorized.package = crate::store::plugins::get(&fixture.database.connection, "reminders")
        .unwrap()
        .unwrap()
        .summary
        .reference();
    let queue = queue::read(&fixture.database.connection, fixture.session).unwrap();
    let session = fixture.session;
    rejects(
        &mut fixture,
        &unauthorized,
        vec![Operation::QueuePause {
            session,
            expected_revision: queue.revision,
            paused: false,
        }],
        ErrorCode::PermissionDenied,
    );
}

#[test]
fn subsequent_user_input_withdraws_resumed_work() {
    let (mut fixture, context) = setup();
    queue::pause(&fixture.database.connection, fixture.session, true).unwrap();
    let turn = admit(&mut fixture, &context);
    let held = fixture.queued();
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
    let run = runs::get(&fixture.database.connection, turn).unwrap();
    assert_eq!(run.status, Status::Cancelled);
    assert_eq!(run.error.unwrap().code, ErrorCode::Conflict);
    let queue = queue::read(&fixture.database.connection, fixture.session).unwrap();
    assert_eq!(queue.items.len(), 1);
    assert_eq!(queue.items[0].turn, held);
    assert!(!queue.items[0].ready);
}

#[test]
fn revised_goal_withdraws_resumed_work() {
    let (mut fixture, context) = setup();
    queue::pause(&fixture.database.connection, fixture.session, true).unwrap();
    let turn = admit(&mut fixture, &context);
    execute(
        &mut fixture,
        &context,
        Command::WritePluginConversationValue {
            key: "goal".into(),
            value: value("paused"),
            expected_revision: 2,
        },
    )
    .unwrap();
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
    let run = runs::get(&fixture.database.connection, turn).unwrap();
    assert_eq!(run.status, Status::Cancelled);
    assert_eq!(run.error.unwrap().code, ErrorCode::RevisionConflict);
}
