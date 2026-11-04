use super::super::fixture::{finish, invoke, setup, start};
use super::*;
use serde_json::json;

#[tokio::test]
async fn preserves_parent_ownership() {
    let mut fixture = setup().await;
    for (index, scenario) in [
        "ownership_survives_parent_continuation",
        "rejects_foreign_and_nested_control",
        "reads_final_answer_from_canonical_history",
        "removed_guidance_keeps_running_recipient",
        "retained_direction_reports_paused",
        "reads_prior_execution_after_child_reuse",
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            fixture.fresh_session().await;
        }
        eprintln!("scenario: {scenario}");
        match scenario {
            "ownership_survives_parent_continuation" => {
                Box::pin(async {
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
                    assert_eq!(state.run.turn, child.turn.id, "{scenario}");
                    assert_eq!(state.run.status, Status::Running, "{scenario}");
                    assert_eq!(
                        origin(&fixture.database.connection, child.turn.id)
                            .await
                            .unwrap()
                            .unwrap()
                            .turn,
                        first,
                        "{scenario}"
                    );
                    assert_eq!(state.response, None, "{scenario}");
                })
                .await
            }
            "rejects_foreign_and_nested_control" => {
                Box::pin(async {
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
                        ErrorCode::PermissionDenied,
                        "{scenario}"
                    );
                    let session = crate::store::commands::session(
                        &fixture.database.connection,
                        fixture.session,
                    )
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
                        panic!("{scenario}: session expected")
                    };
                    fixture.session = other.id;
                    let other = start(&mut fixture).await;
                    assert_eq!(
                        owned(&fixture.database.connection, other, child.turn.session)
                            .await
                            .unwrap_err()
                            .code,
                        ErrorCode::PermissionDenied,
                        "{scenario}"
                    );
                    finish(&mut fixture, other).await;
                    assert_eq!(
                        owned(&fixture.database.connection, other, child.turn.session)
                            .await
                            .unwrap_err()
                            .code,
                        ErrorCode::Conflict,
                        "{scenario}"
                    );
                })
                .await
            }
            "reads_final_answer_from_canonical_history" => {
                Box::pin(async {
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
                    assert_eq!(state.run.status, Status::Completed, "{scenario}");
                    assert_eq!(
                        state.response.as_deref(),
                        Some("Final child evidence 中文 🙂"),
                        "{scenario}"
                    );
                })
                .await
            }
            "removed_guidance_keeps_running_recipient" => {
                Box::pin(async {
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
                    let Output::QueuedTurn(queued) =
                        super::super::fixture::execute(&mut fixture, request)
                            .await
                            .unwrap()
                    else {
                        panic!("{scenario}: queued direction expected")
                    };
                    runs::stop(&fixture.database.connection, queued.id)
                        .await
                        .unwrap();
                    let state = state(&fixture.database.connection, parent, child.turn.session)
                        .await
                        .unwrap();
                    assert_eq!(state.run.turn, child.turn.id, "{scenario}");
                    assert_eq!(state.run.status, Status::Running, "{scenario}");
                })
                .await
            }
            "retained_direction_reports_paused" => {
                Box::pin(async {
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
                    let Output::QueuedTurn(queued) =
                        super::super::fixture::execute(&mut fixture, request)
                            .await
                            .unwrap()
                    else {
                        panic!("{scenario}: queued direction expected")
                    };
                    runs::stop(&fixture.database.connection, child.turn.id)
                        .await
                        .unwrap();
                    finish(&mut fixture, child.turn.id).await;
                    let state = state(&fixture.database.connection, parent, child.turn.session)
                        .await
                        .unwrap();
                    assert_eq!(state.run.turn, queued.id, "{scenario}");
                    assert_eq!(state.run.status, Status::Queued, "{scenario}");
                    assert!(state.paused, "{scenario}");
                })
                .await
            }
            "reads_prior_execution_after_child_reuse" => {
                Box::pin(async {
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
                    assert_ne!(first.turn.id, second.turn.id, "{scenario}");
                    assert_eq!(first.turn.session, second.turn.session, "{scenario}");
                    let latest = state(&fixture.database.connection, parent, first.turn.session)
                        .await
                        .unwrap();
                    assert_eq!(latest.run.turn, second.turn.id, "{scenario}");
                    assert_eq!(latest.run.status, Status::Running, "{scenario}");
                    let prior = state_at(
                        &fixture.database.connection,
                        parent,
                        first.turn.session,
                        Some(first.turn.id),
                    )
                    .await
                    .unwrap();
                    assert_eq!(prior.run.turn, first.turn.id, "{scenario}");
                    assert_eq!(prior.run.status, Status::Completed, "{scenario}");
                    assert_eq!(
                        prior.response.as_deref(),
                        Some("First execution evidence"),
                        "{scenario}"
                    );
                    let consumed = super::super::super::background::consume_child(
                        &fixture.database.connection,
                        parent,
                        first.turn.session,
                        first.turn.id,
                    )
                    .await
                    .unwrap();
                    assert_eq!(consumed.len(), 1, "{scenario}");
                    assert_eq!(
                        consumed[0].state,
                        sailry_protocol::conversation::background::State::Consumed,
                        "{scenario}"
                    );
                    let tasks = super::super::super::background::list(
                        &fixture.database.connection,
                        fixture.session,
                    )
                    .await
                    .unwrap();
                    let first_id = format!("agent:{}", first.turn.id);
                    let second_id = format!("agent:{}", second.turn.id);
                    assert_eq!(
                        tasks.iter().find(|task| task.id == first_id).unwrap().state,
                        sailry_protocol::conversation::background::State::Consumed,
                        "{scenario}"
                    );
                    assert_eq!(
                        tasks
                            .iter()
                            .find(|task| task.id == second_id)
                            .unwrap()
                            .state,
                        sailry_protocol::conversation::background::State::Running,
                        "{scenario}"
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
                        ErrorCode::PermissionDenied,
                        "{scenario}"
                    );
                })
                .await
            }
            _ => unreachable!(),
        }
    }
}
