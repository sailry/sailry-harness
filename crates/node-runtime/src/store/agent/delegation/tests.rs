use super::*;
use crate::agent::canonical::{BlockKind, Role as MessageRole};
use crate::store::agent::tests::fixture::Fixture;
use crate::store::agent::tests::fixture::{message, request as tool_request};
use serde_json::json;

#[tokio::test]
async fn preserves_admission_authority() {
    let mut fixture = Fixture::new().await;
    for (index, scenario) in [
        "model_execution_rejects_admission",
        "canonical_admission_and_recovery",
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            fixture.fresh_session().await;
        }
        eprintln!("scenario: {scenario}");
        match scenario {
            "model_execution_rejects_admission" => Box::pin(async {
                super::super::tests::command(
                    &mut fixture.database,
                    &fixture.events,
                    Command::SetSessionExecution {
                        session: fixture.session,
                        expected_revision: 1,
                        execution: sailry_protocol::conversation::Execution::Model,
                        team: None,
                        models: Default::default(),
                    },
                )
                .await;
                let parent = fixture.queued().await;
                runs::start(&fixture.database.connection, parent)
                    .await
                    .unwrap();
                runs::claim(&mut fixture.database, &fixture.events)
                    .await
                    .unwrap()
                    .unwrap();
                let request = ToolIdentity {
                    name: crate::agent::delegation::tools::SPAWN.into(),
                    id: Some("model-call".into()),
                    arguments: json!({"task":"Delegate"}),
                };
                let error = admit(
                    &mut fixture.database,
                    parent,
                    &request,
                    request.arguments.clone(),
                    None,
                    &fixture.events,
                )
                .await
                .err()
                .expect("model execution cannot delegate");
                assert_eq!(error.code, ErrorCode::PermissionDenied, "{scenario}");
            }).await,
            "canonical_admission_and_recovery" => Box::pin(async {
                let parent = fixture.queued().await;
                runs::start(&fixture.database.connection, parent)
                    .await
                    .unwrap();
                runs::claim(&mut fixture.database, &fixture.events)
                    .await
                    .unwrap()
                    .unwrap();
                let request = ToolIdentity {
                    name: crate::agent::delegation::tools::SPAWN.to_owned(),
                    id: Some("child-call".into()),
                    arguments: json!({"role": null, "title": "Inspect files", "task": "Independent task"}),
                };
                assert!(
                    admit(
                        &mut fixture.database,
                        parent,
                        &request,
                        request.arguments.clone(),
                        None,
                        &fixture.events
                    )
                    .await
                    .is_err()
                , "{scenario}");
                let mut event = message(
        "assistant",
                    MessageRole::Assistant,
                    vec![BlockKind::ToolRequest(tool_request(
                        request.id.as_deref().expect("call identity"),
                        &request.name,
                        request.arguments.clone(),
                    ))],
                );
                event.turn = parent;
                history::append(
                    &mut fixture.database,
                    fixture.session,
                    parent,
                    event,
                    &fixture.events,
                )
                .await
                .unwrap();
                let mut forged = request.clone();
                forged.arguments["task"] = json!("Changed task");
                assert!(
                    admit(
                        &mut fixture.database,
                        parent,
                        &forged,
                        forged.arguments.clone(),
                        None,
                        &fixture.events
                    )
                    .await
                    .is_err()
                , "{scenario}");
                let count = async |db: &Connection| {
                    db.prepare("SELECT COUNT(*) FROM requests")
                        .await
                        .unwrap()
                        .query_row(())
                        .await
                        .and_then(|row| row.get::<i64>(0))
                        .unwrap()
                };
                let requests = count(&fixture.database.connection).await;
                let child = admit(
                    &mut fixture.database,
                    parent,
                    &request,
                    request.arguments.clone(),
                    None,
                    &fixture.events,
                )
                .await
                .unwrap();
                assert_eq!(count(&fixture.database.connection).await, requests, "{scenario}");
                assert_eq!(child.message.text, "Independent task", "{scenario}");
                assert_eq!(child.turn.config.effort, Effort::Low, "{scenario}");
                assert_eq!(child.turn.config.permission, Permission::Ask, "{scenario}");
                assert!(child.child.unwrap().role.is_none(), "{scenario}");
                let captured = history::pages::read(&fixture.database.connection, child.turn.session, None, 20)
                    .await
                    .unwrap();
                assert!(captured.page.entries.is_empty(), "{scenario}");
                assert_eq!(captured.page.runs[0].status, Status::Queued, "{scenario}");
                assert_eq!(
                    captured.page.runs[0].input.as_ref().unwrap().message.text,
        "Independent task"
                , "{scenario}");
                assert!(
                    runs::claim(&mut fixture.database, &fixture.events)
                        .await
                        .unwrap()
                        .is_none()
                , "{scenario}");
                assert_eq!(
                    admit(
                        &mut fixture.database,
                        parent,
                        &request,
                        request.arguments.clone(),
                        None,
                        &fixture.events
                    )
                    .await
                    .err()
                    .unwrap()
                    .code,
                    ErrorCode::Conflict
                , "{scenario}");
                assert!(
                    history::pages::read(&fixture.database.connection, child.turn.session, None, 20)
                        .await
                        .unwrap()
                        .page
                        .entries
                        .is_empty()
                , "{scenario}");
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
                assert_eq!(
                    runs::get(&fixture.database.connection, parent)
                        .await
                        .unwrap()
                        .status,
                    Status::Interrupted
                , "{scenario}");
                let recovered =
                    history::pages::read(&fixture.database.connection, child.turn.session, None, 20)
                        .await
                        .unwrap();
                assert_eq!(recovered.page.runs[0].input, captured.page.runs[0].input, "{scenario}");
                assert_eq!(
                    runs::get(&fixture.database.connection, child.turn.id)
                        .await
                        .unwrap()
                        .status,
                    Status::Interrupted
                , "{scenario}");
                assert!(
                    runs::claim(&mut fixture.database, &fixture.events)
                        .await
                        .unwrap()
                        .is_none()
                , "{scenario}");
                assert!(
                    admit(
                        &mut fixture.database,
                        parent,
                        &request,
                        request.arguments.clone(),
                        None,
                        &fixture.events
                    )
                    .await
                    .is_err()
                , "{scenario}");
                assert!(
                    read(&fixture.database.connection, child.turn.session)
                        .await
                        .unwrap()
                        .is_some()
                , "{scenario}");
            }).await,
            _ => unreachable!(),
        }
    }
}
