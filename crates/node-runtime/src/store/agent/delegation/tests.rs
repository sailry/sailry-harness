use super::*;
use crate::store::agent::tests::fixture::Fixture;
use serde_json::json;

#[test]
fn canonical_admission_and_recovery() {
    let mut fixture = Fixture::new();
    crate::store::agent::tests::fixture::package(
        &fixture.database,
        fixture._root.path(),
        "delegation",
    );
    let parent = fixture.queued();
    runs::start(&fixture.database.connection, parent).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    let request = ToolConfirmationRequest {
        tool_name: crate::plugins::tools::alias("delegation", "spawn_agent"),
        function_call_id: Some("child-call".into()),
        args: json!({"role": null, "title": "Inspect files", "task": "Independent task"}),
    };
    assert!(
        admit(
            &mut fixture.database,
            parent,
            &request,
            request.args.clone(),
            None,
            &fixture.events
        )
        .is_err()
    );
    let mut event = AdkEvent::new("parent-invocation");
    event.author = "assistant".into();
    event.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![adk_core::Part::FunctionCall {
            id: request.function_call_id.clone(),
            name: request.tool_name.clone(),
            args: request.args.clone(),
            thought_signature: None,
        }],
    });
    history::append(
        &mut fixture.database,
        fixture.session,
        parent,
        event,
        &fixture.events,
    )
    .unwrap();
    let mut forged = request.clone();
    forged.args["task"] = json!("Changed task");
    assert!(
        admit(
            &mut fixture.database,
            parent,
            &forged,
            forged.args.clone(),
            None,
            &fixture.events
        )
        .is_err()
    );
    let count = |db: &Connection| {
        db.query_row("SELECT COUNT(*) FROM requests", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    };
    let requests = count(&fixture.database.connection);
    let child = admit(
        &mut fixture.database,
        parent,
        &request,
        request.args.clone(),
        None,
        &fixture.events,
    )
    .unwrap();
    assert_eq!(count(&fixture.database.connection), requests);
    assert_eq!(child.message.text, "Independent task");
    assert_eq!(child.turn.config.effort, Effort::Low);
    assert_eq!(child.turn.config.permission, Permission::Ask);
    assert!(child.child.unwrap().role.is_none());
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        admit(
            &mut fixture.database,
            parent,
            &request,
            request.args.clone(),
            None,
            &fixture.events
        )
        .err()
        .unwrap()
        .code,
        ErrorCode::Conflict
    );
    assert!(
        history::pages::read(&fixture.database.connection, child.turn.session, None, 20)
            .unwrap()
            .page
            .entries
            .is_empty()
    );
    runs::recover(&fixture.database.connection).unwrap();
    assert_eq!(
        runs::get(&fixture.database.connection, parent)
            .unwrap()
            .status,
        Status::Interrupted
    );
    assert_eq!(
        runs::get(&fixture.database.connection, child.turn.id)
            .unwrap()
            .status,
        Status::Interrupted
    );
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
    assert!(
        admit(
            &mut fixture.database,
            parent,
            &request,
            request.args.clone(),
            None,
            &fixture.events
        )
        .is_err()
    );
    assert!(
        read(&fixture.database.connection, child.turn.session)
            .unwrap()
            .is_some()
    );
}
