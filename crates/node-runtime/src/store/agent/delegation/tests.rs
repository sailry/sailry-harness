use super::*;
use crate::agent::canonical::{BlockKind, Role as MessageRole};
use crate::store::agent::tests::fixture::Fixture;
use crate::store::agent::tests::fixture::{message, request as tool_request};
use serde_json::json;

#[tokio::test]
async fn canonical_admission_and_recovery() {
    let mut fixture = Fixture::new().await;
    crate::store::agent::tests::fixture::package(
        &fixture.database,
        fixture._root.path(),
        "delegation",
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
        name: crate::plugins::tools::alias("delegation", "spawn_agent"),
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
    );
    let mut event = message(
        "assistant",
        MessageRole::Assistant,
        vec![BlockKind::ToolRequest(tool_request(
            request.id.as_deref(),
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
    );
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
    assert_eq!(count(&fixture.database.connection).await, requests);
    assert_eq!(child.message.text, "Independent task");
    assert_eq!(child.turn.config.effort, Effort::Low);
    assert_eq!(child.turn.config.permission, Permission::Ask);
    assert!(child.child.unwrap().role.is_none());
    let captured = history::pages::read(&fixture.database.connection, child.turn.session, None, 20)
        .await
        .unwrap();
    assert!(captured.page.entries.is_empty());
    assert_eq!(captured.page.runs[0].status, Status::Queued);
    assert_eq!(
        captured.page.runs[0].input.as_ref().unwrap().message.text,
        "Independent task"
    );
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
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
    );
    assert!(
        history::pages::read(&fixture.database.connection, child.turn.session, None, 20)
            .await
            .unwrap()
            .page
            .entries
            .is_empty()
    );
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
    );
    let recovered =
        history::pages::read(&fixture.database.connection, child.turn.session, None, 20)
            .await
            .unwrap();
    assert_eq!(recovered.page.runs[0].input, captured.page.runs[0].input);
    assert_eq!(
        runs::get(&fixture.database.connection, child.turn.id)
            .await
            .unwrap()
            .status,
        Status::Interrupted
    );
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
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
    );
    assert!(
        read(&fixture.database.connection, child.turn.session)
            .await
            .unwrap()
            .is_some()
    );
}
