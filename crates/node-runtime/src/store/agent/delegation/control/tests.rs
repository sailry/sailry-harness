use super::super::fixture::{finish, invoke, setup, start};
use super::*;
use serde_json::json;

#[test]
fn ownership_survives_parent_continuation() {
    let mut fixture = setup();
    let first = start(&mut fixture);
    let child = invoke(
        &mut fixture,
        first,
        json!({"task":"Inspect","background":true}),
    )
    .unwrap();
    finish(&mut fixture, first);
    let next = start(&mut fixture);
    let state = state(&fixture.database.connection, next, child.turn.session).unwrap();
    assert_eq!(state.run.turn, child.turn.id);
    assert_eq!(state.run.status, Status::Running);
    assert_eq!(
        origin(&fixture.database.connection, child.turn.id)
            .unwrap()
            .unwrap()
            .turn,
        first
    );
    assert_eq!(state.response, None);
}

#[test]
fn rejects_foreign_and_nested_control() {
    let mut fixture = setup();
    let parent = start(&mut fixture);
    let child = invoke(
        &mut fixture,
        parent,
        json!({"task":"Inspect","background":true}),
    )
    .unwrap();
    assert_eq!(
        owned(
            &fixture.database.connection,
            child.turn.id,
            child.turn.session
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    let session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    let Output::Session(other) = crate::store::agent::tests::command(
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
    fixture.session = other.id;
    let other = start(&mut fixture);
    assert_eq!(
        owned(&fixture.database.connection, other, child.turn.session)
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    finish(&mut fixture, other);
    assert_eq!(
        owned(&fixture.database.connection, other, child.turn.session)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[test]
fn reads_final_answer_from_canonical_history() {
    let mut fixture = setup();
    let parent = start(&mut fixture);
    let child = invoke(
        &mut fixture,
        parent,
        json!({"task":"Inspect","background":true}),
    )
    .unwrap();
    let mut event = AdkEvent::new("child-answer");
    event.author = "assistant".into();
    event.set_content(adk_core::Content::new("model").with_text("Final child evidence 中文 🙂"));
    history::append(
        &mut fixture.database,
        child.turn.session,
        child.turn.id,
        event,
        &fixture.events,
    )
    .unwrap();
    finish(&mut fixture, child.turn.id);
    let state = state(&fixture.database.connection, parent, child.turn.session).unwrap();
    assert_eq!(state.run.status, Status::Completed);
    assert_eq!(
        state.response.as_deref(),
        Some("Final child evidence 中文 🙂")
    );
}

#[test]
fn removed_guidance_keeps_running_recipient() {
    let mut fixture = setup();
    let parent = start(&mut fixture);
    let child = invoke(
        &mut fixture,
        parent,
        json!({"task":"Inspect","background":true}),
    )
    .unwrap();
    let request = Request::new(
        fixture.database.node,
        Command::GuideTurn {
            recipient: child.turn.id,
            expected_revision: child.turn.revision,
            message: "Change direction".into(),
        },
    );
    let Output::QueuedTurn(queued) = super::super::fixture::execute(&mut fixture, request).unwrap()
    else {
        panic!("queued direction expected")
    };
    runs::stop(&fixture.database.connection, queued.id).unwrap();
    let state = state(&fixture.database.connection, parent, child.turn.session).unwrap();
    assert_eq!(state.run.turn, child.turn.id);
    assert_eq!(state.run.status, Status::Running);
}

#[test]
fn retained_direction_reports_paused() {
    let mut fixture = setup();
    let parent = start(&mut fixture);
    let child = invoke(
        &mut fixture,
        parent,
        json!({"task":"Inspect","background":true}),
    )
    .unwrap();
    let request = Request::new(
        fixture.database.node,
        Command::GuideTurn {
            recipient: child.turn.id,
            expected_revision: child.turn.revision,
            message: "Change direction".into(),
        },
    );
    let Output::QueuedTurn(queued) = super::super::fixture::execute(&mut fixture, request).unwrap()
    else {
        panic!("queued direction expected")
    };
    runs::stop(&fixture.database.connection, child.turn.id).unwrap();
    finish(&mut fixture, child.turn.id);
    let state = state(&fixture.database.connection, parent, child.turn.session).unwrap();
    assert_eq!(state.run.turn, queued.id);
    assert_eq!(state.run.status, Status::Queued);
    assert!(state.paused);
}
