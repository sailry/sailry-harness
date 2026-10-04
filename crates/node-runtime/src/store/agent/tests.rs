use super::*;

mod attention;
mod compaction;
mod continuation;
mod conversation_values;
pub(super) mod fixture;
mod forks;
mod images;
mod location;
mod preview;
mod rewind;
mod title;

pub(super) fn command(
    database: &mut Database,
    events: &broadcast::Sender<EventEnvelope>,
    command: Command,
) -> Output {
    let (reply, response) = oneshot::channel();
    assert!(
        database
            .dispatch(
                database.node,
                Request::new(database.node, command),
                reply,
                events
            )
            .is_none()
    );
    response
        .blocking_recv()
        .unwrap()
        .unwrap()
        .completion
        .blocking_recv()
        .unwrap()
        .unwrap()
}

#[test]
fn rebuilds_from_durable_events() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().canonicalize().unwrap().join("node.sqlite3");
    let node = NodeId([31; 32]);
    let mut database = Database::open(&path, node, None).unwrap();
    let (events, _) = broadcast::channel(64);
    let Output::Project(project) = command(
        &mut database,
        &events,
        Command::RegisterProject {
            name: "History fixture".into(),
            path: fixture.path().to_str().unwrap().into(),
        },
    ) else {
        panic!("project expected")
    };
    let config = SessionConfig {
        assistant: None,
        resource: None,
        provider: ProviderId::new(),
        model: "fixture".into(),
        effort: Effort::Low,
        mode: sailry_protocol::WorkMode::Code,
        permission: sailry_protocol::Permission::Ask,
        credential: None,
    };
    let Output::Session(session) = command(
        &mut database,
        &events,
        Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(config),
        },
    ) else {
        panic!("session expected")
    };
    let Output::QueuedTurn(turn) = command(
        &mut database,
        &events,
        Command::QueueTurn {
            session: session.id,
            expected_revision: 1,
            message: "fixture prompt".into(),
        },
    ) else {
        panic!("turn expected")
    };
    runs::start(&database.connection, turn.id).unwrap();
    assert_eq!(
        runs::claim(&mut database, &events)
            .unwrap()
            .unwrap()
            .turn
            .id,
        turn.id
    );
    let mut updates = events.subscribe();
    let mut event = AdkEvent::new("fixture-invocation");
    event.author = "assistant".into();
    event.set_content(adk_core::Content::new("model").with_text("persisted answer"));
    event.llm_request = Some("request copy".into());
    event.llm_response.error_message = Some("sensitive upstream diagnostic fixture".into());
    event.provider_metadata.insert(
        "gcp.vertex.agent.llm_response".into(),
        "response copy".into(),
    );
    event
        .provider_metadata
        .insert("gcp.vertex.agent.llm_request".into(), "request copy".into());
    event.actions.state_delta.extend([
        ("value".into(), serde_json::json!(1)),
        ("app:shared".into(), serde_json::json!(2)),
        ("user:shared".into(), serde_json::json!(3)),
        ("temp:ephemeral".into(), serde_json::json!(4)),
    ]);
    history::append(&mut database, session.id, turn.id, event.clone(), &events).unwrap();
    updates.try_recv().unwrap();
    history::append(&mut database, session.id, turn.id, event.clone(), &events).unwrap();
    assert!(updates.try_recv().is_err());
    let state = history::state(&database.connection, session.id).unwrap();
    assert_eq!(state.len(), 3);
    assert_eq!(state["value"], 1);
    database
        .connection
        .execute("DELETE FROM agent_state", [])
        .unwrap();
    history::rebuild(&mut database.connection).unwrap();
    assert_eq!(
        history::state(&database.connection, session.id).unwrap(),
        state
    );
    runs::finish(&mut database, turn.id, Status::Completed, None, &events).unwrap();
    let original = history::pages::read(&database.connection, session.id, None, 100).unwrap();
    assert_eq!(original.page.entries.len(), 1);
    event.set_content(adk_core::Content::new("model").with_text("different answer"));
    assert!(history::append(&mut database, session.id, turn.id, event.clone(), &events).is_err());
    event.id = "partial-fixture".into();
    event.llm_response.partial = true;
    assert!(history::append(&mut database, session.id, turn.id, event.clone(), &events).is_err());
    event.llm_response.partial = false;
    assert!(history::append(&mut database, SessionId::new(), turn.id, event, &events).is_err());
    assert_eq!(
        history::pages::read(&database.connection, session.id, None, 100).unwrap(),
        original
    );
    let request = GetRequest {
        app_name: APP.into(),
        user_id: USER.into(),
        session_id: session.id.to_string(),
        num_recent_events: Some(1),
        after: None,
    };
    let recent = sessions::get(&database.connection, turn.id, request.clone()).unwrap();
    assert_eq!(recent.events().len(), 1);
    assert!(recent.events().at(0).unwrap().llm_request.is_none());
    assert_eq!(
        recent
            .events()
            .at(0)
            .unwrap()
            .llm_response
            .error_message
            .as_deref(),
        Some("model response reported an error")
    );
    assert!(
        !recent
            .events()
            .at(0)
            .unwrap()
            .provider_metadata
            .contains_key("gcp.vertex.agent.llm_response")
    );
    assert!(
        !recent
            .events()
            .at(0)
            .unwrap()
            .provider_metadata
            .contains_key("gcp.vertex.agent.llm_request")
    );
    assert_eq!(recent.state().all(), state);
    let empty = sessions::get(
        &database.connection,
        turn.id,
        GetRequest {
            num_recent_events: Some(0),
            ..request.clone()
        },
    )
    .unwrap();
    assert_eq!(empty.events().len(), 0);
    assert_eq!(empty.last_update_time(), recent.last_update_time());
    assert!(sessions::get(&database.connection, TurnId::new(), request.clone()).is_err());
    assert!(
        sessions::get(
            &database.connection,
            turn.id,
            GetRequest {
                user_id: "another-owner".into(),
                ..request
            }
        )
        .is_err()
    );
    database.close().unwrap();
    let database = Database::open(&path, node, None).unwrap();
    assert_eq!(
        history::pages::read(&database.connection, session.id, None, 100).unwrap(),
        original
    );
    assert_eq!(
        history::state(&database.connection, session.id).unwrap(),
        state
    );
    database.close().unwrap();
}
