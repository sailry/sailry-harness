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

async fn setup() -> (Fixture, plugin::Context) {
    let mut fixture = Fixture::new().await;
    let context = prepare(&mut fixture).await;
    execute(
        &mut fixture,
        &context,
        Command::WritePluginConversationValue {
            key: "goal".into(),
            value: value("paused"),
            expected_revision: 0,
        },
    )
    .await
    .unwrap();
    (fixture, context)
}

async fn goal(fixture: &Fixture) -> ConversationEntry {
    crate::store::plugins::storage::conversation::read(
        &fixture.database.connection,
        "goals",
        fixture.session,
        "goal",
    )
    .await
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

async fn admit(fixture: &mut Fixture, context: &plugin::Context) -> TurnId {
    let queue = queue::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let operations = operations(
        fixture.session,
        queue.revision,
        goal(fixture).await.revision,
    );
    let Output::PluginTransaction(outputs) =
        execute(fixture, context, Command::PluginTransaction { operations })
            .await
            .unwrap()
    else {
        panic!("transaction expected")
    };
    let Output::QueuedTurn(turn) = &outputs[2] else {
        panic!("turn expected")
    };
    turn.id
}

async fn rejects(
    fixture: &mut Fixture,
    context: &plugin::Context,
    operations: Vec<Operation>,
    code: ErrorCode,
) {
    let queue = queue::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let state = goal(fixture).await;
    let runs = runs::list(&fixture.database.connection, fixture.session, &[])
        .await
        .unwrap();
    assert_eq!(
        execute(fixture, context, Command::PluginTransaction { operations })
            .await
            .unwrap_err()
            .code,
        code
    );
    assert_eq!(
        queue::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap(),
        queue
    );
    assert_eq!(goal(fixture).await, state);
    assert_eq!(
        runs::list(&fixture.database.connection, fixture.session, &[])
            .await
            .unwrap(),
        runs
    );
}

#[tokio::test]
async fn stopped_queue_preserves_fence_and_marker() {
    let (mut fixture, context) = setup().await;
    let original = fixture.queued().await;
    runs::start(&fixture.database.connection, original)
        .await
        .unwrap();
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap()
            .turn
            .id,
        original
    );
    runs::stop(&fixture.database.connection, original)
        .await
        .unwrap();
    runs::finish(
        &mut fixture.database,
        original,
        Status::Cancelled,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    let paused = queue::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert!(paused.paused);
    assert!(paused.items.is_empty());
    assert_eq!(
        continue_turn(&mut fixture, &context, None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );

    let turn = admit(&mut fixture, &context).await;
    let queue = queue::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert!(!queue.paused);
    assert_eq!(queue.items.len(), 1);
    assert_eq!(queue.items[0].turn, turn);
    assert!(queue.items[0].ready);
    assert_eq!(goal(&fixture).await.value["state"], "active");
    assert_eq!(goal(&fixture).await.revision, 2);
    let run = runs::get(&fixture.database.connection, turn).await.unwrap();
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
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invocation.turn.id, turn);
    assert!(invocation.automatic);
    assert_eq!(invocation.message.text, "Continue the requested work");
    assert_eq!(
        runs::get(&fixture.database.connection, turn)
            .await
            .unwrap()
            .continuation,
        run.continuation
    );
}

#[tokio::test]
async fn held_input_rolls_back_state_and_queue() {
    let (mut fixture, context) = setup().await;
    let held = fixture.queued().await;
    queue::pause(&fixture.database.connection, fixture.session, true)
        .await
        .unwrap();
    let queue = queue::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(queue.items[0].turn, held);
    assert!(!queue.items[0].ready);
    let operations = operations(
        fixture.session,
        queue.revision,
        goal(&fixture).await.revision,
    );
    rejects(&mut fixture, &context, operations, ErrorCode::Busy).await;
}

#[tokio::test]
async fn active_work_rolls_back_state_and_queue() {
    for stopping in [false, true] {
        let (mut fixture, context) = setup().await;
        let active = fixture.queued().await;
        runs::start(&fixture.database.connection, active)
            .await
            .unwrap();
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap();
        if stopping {
            runs::stop(&fixture.database.connection, active)
                .await
                .unwrap();
        } else {
            queue::pause(&fixture.database.connection, fixture.session, true)
                .await
                .unwrap();
        }
        let queue = queue::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap();
        let operations = operations(
            fixture.session,
            queue.revision,
            goal(&fixture).await.revision,
        );
        rejects(&mut fixture, &context, operations, ErrorCode::Busy).await;
    }
}

#[tokio::test]
async fn stale_revisions_roll_back_state_and_queue() {
    for stale_queue in [false, true] {
        let (mut fixture, context) = setup().await;
        queue::pause(&fixture.database.connection, fixture.session, true)
            .await
            .unwrap();
        let queue = queue::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap();
        let operations = operations(
            fixture.session,
            queue.revision - u64::from(stale_queue),
            goal(&fixture).await.revision - u64::from(!stale_queue),
        );
        rejects(
            &mut fixture,
            &context,
            operations,
            ErrorCode::RevisionConflict,
        )
        .await;
    }
}

#[tokio::test]
async fn preserves_session_scope_and_capability_checks() {
    let (mut fixture, context) = setup().await;
    queue::pause(&fixture.database.connection, fixture.session, true)
        .await
        .unwrap();
    let current = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let Output::Session(other) = command(
        &mut fixture.database,
        &fixture.events,
        Command::CreateSession {
            project: current.project,
            worktree: Some(current.worktree),
            config: Some(current.config),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    queue::pause(&fixture.database.connection, other.id, true)
        .await
        .unwrap();
    let other_queue = queue::read(&fixture.database.connection, other.id)
        .await
        .unwrap();
    rejects(
        &mut fixture,
        &context,
        vec![Operation::QueuePause {
            session: other.id,
            expected_revision: other_queue.revision,
            paused: false,
        }],
        ErrorCode::PermissionDenied,
    )
    .await;
    assert_eq!(
        queue::read(&fixture.database.connection, other.id)
            .await
            .unwrap(),
        other_queue
    );

    fixture::package(
        &fixture.database,
        &fixture._root.path().join("profile"),
        "reminders",
    )
    .await;
    let mut unauthorized = context.clone();
    unauthorized.package = crate::store::plugins::get(&fixture.database.connection, "reminders")
        .await
        .unwrap()
        .unwrap()
        .summary
        .reference();
    let queue = queue::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
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
    )
    .await;
}

#[tokio::test]
async fn subsequent_user_input_withdraws_resumed_work() {
    let (mut fixture, context) = setup().await;
    queue::pause(&fixture.database.connection, fixture.session, true)
        .await
        .unwrap();
    let turn = admit(&mut fixture, &context).await;
    let held = fixture.queued().await;
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
    let run = runs::get(&fixture.database.connection, turn).await.unwrap();
    assert_eq!(run.status, Status::Cancelled);
    assert_eq!(run.error.unwrap().code, ErrorCode::Conflict);
    let queue = queue::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(queue.items.len(), 1);
    assert_eq!(queue.items[0].turn, held);
    assert!(!queue.items[0].ready);
}

#[tokio::test]
async fn revised_goal_withdraws_resumed_work() {
    let (mut fixture, context) = setup().await;
    queue::pause(&fixture.database.connection, fixture.session, true)
        .await
        .unwrap();
    let turn = admit(&mut fixture, &context).await;
    execute(
        &mut fixture,
        &context,
        Command::WritePluginConversationValue {
            key: "goal".into(),
            value: value("paused"),
            expected_revision: 2,
        },
    )
    .await
    .unwrap();
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
    let run = runs::get(&fixture.database.connection, turn).await.unwrap();
    assert_eq!(run.status, Status::Cancelled);
    assert_eq!(run.error.unwrap().code, ErrorCode::RevisionConflict);
}
