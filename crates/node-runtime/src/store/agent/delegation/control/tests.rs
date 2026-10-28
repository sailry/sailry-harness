use super::super::fixture::{finish, invoke, setup, start};
use super::*;
use serde_json::json;

#[tokio::test]
async fn ownership_survives_parent_continuation() {
    let mut fixture = setup().await;
    let first = start(&mut fixture).await;
    let child = invoke(
        &mut fixture,
        first,
        json!({"task":"Inspect","background":true}),
    )
    .await
    .unwrap();
    finish(&mut fixture, first).await;
    let next = start(&mut fixture).await;
    let state = state(&fixture.database.connection, next, child.turn.session)
        .await
        .unwrap();
    assert_eq!(state.run.turn, child.turn.id);
    assert_eq!(state.run.status, Status::Running);
    assert_eq!(
        origin(&fixture.database.connection, child.turn.id)
            .await
            .unwrap()
            .unwrap()
            .turn,
        first
    );
    assert_eq!(state.response, None);
}

#[tokio::test]
async fn rejects_foreign_and_nested_control() {
    let mut fixture = setup().await;
    let parent = start(&mut fixture).await;
    let child = invoke(
        &mut fixture,
        parent,
        json!({"task":"Inspect","background":true}),
    )
    .await
    .unwrap();
    assert_eq!(
        owned(
            &fixture.database.connection,
            child.turn.id,
            child.turn.session
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    let session = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let Output::Session(other) = crate::store::agent::tests::command(
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
    fixture.session = other.id;
    let other = start(&mut fixture).await;
    assert_eq!(
        owned(&fixture.database.connection, other, child.turn.session)
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    finish(&mut fixture, other).await;
    assert_eq!(
        owned(&fixture.database.connection, other, child.turn.session)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn reads_final_answer_from_canonical_history() {
    let mut fixture = setup().await;
    let parent = start(&mut fixture).await;
    let child = invoke(
        &mut fixture,
        parent,
        json!({"task":"Inspect","background":true}),
    )
    .await
    .unwrap();
    let mut event = crate::store::agent::tests::fixture::message(
        "assistant",
        crate::agent::canonical::Role::Assistant,
        vec![crate::agent::canonical::BlockKind::Text {
            text: "Final child evidence 中文 🙂".into(),
        }],
    );
    event.turn = child.turn.id;
    history::append(
        &mut fixture.database,
        child.turn.session,
        child.turn.id,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
    finish(&mut fixture, child.turn.id).await;
    let state = state(&fixture.database.connection, parent, child.turn.session)
        .await
        .unwrap();
    assert_eq!(state.run.status, Status::Completed);
    assert_eq!(
        state.response.as_deref(),
        Some("Final child evidence 中文 🙂")
    );
}

#[tokio::test]
async fn removed_guidance_keeps_running_recipient() {
    let mut fixture = setup().await;
    let parent = start(&mut fixture).await;
    let child = invoke(
        &mut fixture,
        parent,
        json!({"task":"Inspect","background":true}),
    )
    .await
    .unwrap();
    let request = Request::new(
        fixture.database.node,
        Command::GuideTurn {
            recipient: child.turn.id,
            expected_revision: child.turn.revision,
            message: "Change direction".into(),
        },
    );
    let Output::QueuedTurn(queued) = super::super::fixture::execute(&mut fixture, request)
        .await
        .unwrap()
    else {
        panic!("queued direction expected")
    };
    runs::stop(&fixture.database.connection, queued.id)
        .await
        .unwrap();
    let state = state(&fixture.database.connection, parent, child.turn.session)
        .await
        .unwrap();
    assert_eq!(state.run.turn, child.turn.id);
    assert_eq!(state.run.status, Status::Running);
}

#[tokio::test]
async fn retained_direction_reports_paused() {
    let mut fixture = setup().await;
    let parent = start(&mut fixture).await;
    let child = invoke(
        &mut fixture,
        parent,
        json!({"task":"Inspect","background":true}),
    )
    .await
    .unwrap();
    let request = Request::new(
        fixture.database.node,
        Command::GuideTurn {
            recipient: child.turn.id,
            expected_revision: child.turn.revision,
            message: "Change direction".into(),
        },
    );
    let Output::QueuedTurn(queued) = super::super::fixture::execute(&mut fixture, request)
        .await
        .unwrap()
    else {
        panic!("queued direction expected")
    };
    runs::stop(&fixture.database.connection, child.turn.id)
        .await
        .unwrap();
    finish(&mut fixture, child.turn.id).await;
    let state = state(&fixture.database.connection, parent, child.turn.session)
        .await
        .unwrap();
    assert_eq!(state.run.turn, queued.id);
    assert_eq!(state.run.status, Status::Queued);
    assert!(state.paused);
}

#[tokio::test]
async fn reads_prior_execution_after_child_reuse() {
    let mut fixture = setup().await;
    let parent = start(&mut fixture).await;
    let first = invoke(
        &mut fixture,
        parent,
        json!({"task":"First task","background":true}),
    )
    .await
    .unwrap();
    let mut event = crate::store::agent::tests::fixture::message(
        "assistant",
        crate::agent::canonical::Role::Assistant,
        vec![crate::agent::canonical::BlockKind::Text {
            text: "First execution evidence".into(),
        }],
    );
    event.turn = first.turn.id;
    history::append(
        &mut fixture.database,
        first.turn.session,
        first.turn.id,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
    finish(&mut fixture, first.turn.id).await;
    let second = invoke(
        &mut fixture,
        parent,
        json!({"session":first.turn.session,"task":"Second task","background":true}),
    )
    .await
    .unwrap();
    assert_ne!(first.turn.id, second.turn.id);
    assert_eq!(first.turn.session, second.turn.session);
    let latest = state(&fixture.database.connection, parent, first.turn.session)
        .await
        .unwrap();
    assert_eq!(latest.run.turn, second.turn.id);
    assert_eq!(latest.run.status, Status::Running);
    let prior = state_at(
        &fixture.database.connection,
        parent,
        first.turn.session,
        Some(first.turn.id),
    )
    .await
    .unwrap();
    assert_eq!(prior.run.turn, first.turn.id);
    assert_eq!(prior.run.status, Status::Completed);
    assert_eq!(prior.response.as_deref(), Some("First execution evidence"));
    let consumed = super::super::super::background::consume_child(
        &fixture.database.connection,
        parent,
        first.turn.session,
        first.turn.id,
    )
    .await
    .unwrap();
    assert_eq!(consumed.len(), 1);
    assert_eq!(
        consumed[0].state,
        sailry_protocol::conversation::background::State::Consumed
    );
    let tasks =
        super::super::super::background::list(&fixture.database.connection, fixture.session)
            .await
            .unwrap();
    let first_id = format!("agent:{}", first.turn.id);
    let second_id = format!("agent:{}", second.turn.id);
    assert_eq!(
        tasks.iter().find(|task| task.id == first_id).unwrap().state,
        sailry_protocol::conversation::background::State::Consumed
    );
    assert_eq!(
        tasks
            .iter()
            .find(|task| task.id == second_id)
            .unwrap()
            .state,
        sailry_protocol::conversation::background::State::Running
    );
    assert_eq!(
        state_at(
            &fixture.database.connection,
            parent,
            first.turn.session,
            Some(parent)
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
}
