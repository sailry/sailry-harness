use super::*;
use fixture::Fixture;

fn prepare(fixture: &mut Fixture) -> plugin::Context {
    let profile = fixture._root.path().join("profile");
    std::fs::create_dir(&profile).unwrap();
    fixture::package(&fixture.database, &profile, "goals");
    let package = crate::store::plugins::get(&fixture.database.connection, "goals")
        .unwrap()
        .unwrap();
    let session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    let context = plugin::Context {
        package: package.summary.reference(),
        surface: Default::default(),
        worktree: Some(session.worktree),
        session: Some(session.id),
        turn: None,
        invocation: None,
    };
    execute(
        fixture,
        &context,
        Command::WritePluginValue {
            key: "run/state".into(),
            value: serde_json::json!({"active":true}),
            expected_revision: 0,
        },
    )
    .unwrap();
    context
}

fn execute(
    fixture: &mut Fixture,
    context: &plugin::Context,
    command: Command,
) -> Result<Output, Fault> {
    let (reply, response) = oneshot::channel();
    assert!(
        fixture
            .database
            .dispatch(
                fixture.database.node,
                Request::new(fixture.database.node, command).with_plugin(context.clone()),
                reply,
                &fixture.events
            )
            .is_none()
    );
    response
        .blocking_recv()
        .unwrap()?
        .completion
        .blocking_recv()
        .unwrap()
}

fn continue_turn(
    fixture: &mut Fixture,
    context: &plugin::Context,
    after: Option<TurnId>,
) -> Result<TurnId, Fault> {
    let output = execute(
        fixture,
        context,
        Command::ContinueTurn {
            session: fixture.session,
            after,
            key: "run/state".into(),
            expected_revision: 1,
            scope: plugin::storage::Scope::Node,
            message: "Continue the requested work".into(),
        },
    )?;
    let Output::QueuedTurn(turn) = output else {
        panic!("turn expected")
    };
    Ok(turn.id)
}

#[test]
fn claims_ordinary_input_with_an_opaque_fence() {
    let mut fixture = Fixture::new();
    let context = prepare(&mut fixture);
    let after = fixture.completed(&["Verified work"]);
    let turn = continue_turn(&mut fixture, &context, Some(after)).unwrap();
    let invocation = runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    assert_eq!(invocation.turn.id, turn);
    assert!(invocation.automatic);
    assert_eq!(invocation.message.text, "Continue the requested work");
    assert_eq!(
        continue_turn(&mut fixture, &context, Some(after))
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
}

#[test]
fn restored_conversation_values_do_not_authorize_automatic_input() {
    let mut fixture = Fixture::new();
    let mut context = prepare(&mut fixture);
    let after = fixture.completed(&["Recorded result"]);
    let Output::PluginConversationValue(entry) = execute(
        &mut fixture,
        &context,
        Command::WritePluginConversationValue {
            key: "state".into(),
            value: serde_json::json!({"running":true}),
            expected_revision: 0,
        },
    )
    .unwrap() else {
        panic!("entry expected")
    };
    let source = fixture.session;
    let Output::Session(branch) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: source,
            through: after,
            expected_revision: 1,
        },
    ) else {
        panic!("branch expected")
    };
    context.session = Some(branch.id);
    let continuation = |revision| Command::ContinueTurn {
        session: branch.id,
        after: None,
        key: "state".into(),
        expected_revision: revision,
        scope: plugin::storage::Scope::Conversation,
        message: "Continue the requested work".into(),
    };
    assert_eq!(
        execute(&mut fixture, &context, continuation(1))
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let Output::PluginConversationValue(resumed) = execute(
        &mut fixture,
        &context,
        Command::WritePluginConversationValue {
            key: "state".into(),
            value: entry.value,
            expected_revision: 1,
        },
    )
    .unwrap() else {
        panic!("entry expected")
    };
    assert!(!resumed.restored);
    assert!(execute(&mut fixture, &context, continuation(resumed.revision)).is_ok());
}

#[test]
fn held_user_input_supersedes_automatic_input() {
    let mut fixture = Fixture::new();
    let context = prepare(&mut fixture);
    let turn = continue_turn(&mut fixture, &context, None).unwrap();
    let held = fixture.queued();
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
    let run = runs::get(&fixture.database.connection, turn).unwrap();
    assert_eq!(run.status, Status::Cancelled);
    assert_eq!(run.error.unwrap().code, ErrorCode::Conflict);
    runs::start(&fixture.database.connection, held).unwrap();
    let invocation = runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    assert_eq!(invocation.turn.id, held);
    assert!(!invocation.automatic);
}

#[test]
fn changed_private_revision_withdraws_automatic_input() {
    let mut fixture = Fixture::new();
    let context = prepare(&mut fixture);
    let turn = continue_turn(&mut fixture, &context, None).unwrap();
    execute(
        &mut fixture,
        &context,
        Command::WritePluginValue {
            key: "run/state".into(),
            value: serde_json::json!({"active":false}),
            expected_revision: 1,
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

#[test]
fn refuses_pending_paused_and_plan_sessions() {
    let mut fixture = Fixture::new();
    let context = prepare(&mut fixture);
    let held = fixture.queued();
    assert_eq!(
        continue_turn(&mut fixture, &context, None)
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    runs::stop(&fixture.database.connection, held).unwrap();
    queue::pause(&fixture.database.connection, fixture.session, true).unwrap();
    assert_eq!(
        continue_turn(&mut fixture, &context, None)
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    queue::pause(&fixture.database.connection, fixture.session, false).unwrap();
    let mut session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    session.config.mode = WorkMode::Plan;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionConfig {
            session: session.id,
            expected_revision: session.revision,
            config: session.config,
        },
    );
    assert_eq!(
        continue_turn(&mut fixture, &context, None)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[test]
fn restart_does_not_replay_queued_automatic_input() {
    let mut fixture = Fixture::new();
    let context = prepare(&mut fixture);
    let turn = continue_turn(&mut fixture, &context, None).unwrap();
    continuation::recover(&fixture.database.connection).unwrap();
    assert_eq!(
        runs::get(&fixture.database.connection, turn)
            .unwrap()
            .status,
        Status::Cancelled
    );
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
}

#[test]
fn disabling_the_owner_withdraws_automatic_input() {
    let mut fixture = Fixture::new();
    let context = prepare(&mut fixture);
    let turn = continue_turn(&mut fixture, &context, None).unwrap();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetPluginEnabled {
            name: "goals".into(),
            expected_revision: 1,
            enabled: false,
        },
    );
    assert_eq!(
        runs::get(&fixture.database.connection, turn)
            .unwrap()
            .status,
        Status::Cancelled
    );
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
}
