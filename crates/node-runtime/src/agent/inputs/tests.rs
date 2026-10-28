use super::*;
use adk_session::SessionService;
use sailry_client::Client;
use sailry_protocol::{Command, Output, Permission, ProviderId, SessionConfig, WorkMode};

async fn running(node: &crate::Node) -> (SessionId, TurnId) {
    let client = Client::new(node.local());
    let Output::Session(session) = client
        .execute(client.prepare(Command::CreateSession {
            project: None,
            worktree: None,
            config: Some(SessionConfig {
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Default::default(),
                features: Default::default(),
                mode: WorkMode::Code,
                options: Default::default(),
                permission: Permission::Ask,
                credential: None,
            }),
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Original user text".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    let owner = turn.id;
    node.with_storage(move |db| {
        Box::pin(async move {
            assert_eq!(
                db.execute(
                    "UPDATE agent_runs SET body=CAST(json_set(CAST(body AS TEXT),'$.status','running') AS BLOB) WHERE turn=?1",
                    [owner.to_string()],
                )
                .await
                .map_err(|error| Fault::new(ErrorCode::Internal, error.to_string()))?,
                1
            );
            Ok(())
        })
    })
    .await
    .unwrap();
    node.ingress
        .agents
        .register(owner, session.id, CancellationToken::new());
    (session.id, owner)
}

#[tokio::test]
async fn writes_context_before_new_input() {
    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let (session, owner) = running(&node).await;
    let batch = Batch {
        entries: vec![Entry {
            turn: owner,
            references: vec![],
            content: Content::new("user").with_text("Original user text"),
            capability: None,
        }],
    };
    let mut context = Event::new("context-fixture");
    context.author = "system".into();
    context
        .provider_metadata
        .insert(prompt::session::METADATA.into(), "context".into());
    context.set_content(Content::new("user").with_text("Changed runtime context"));
    context.actions.state_delta.insert(
        prompt::session::POLICY.into(),
        serde_json::json!("Changed policy"),
    );
    batch
        .append(
            &node.ingress,
            session,
            owner,
            "fixture",
            "user",
            None,
            Some(context.clone()),
        )
        .await
        .unwrap();
    let history = node
        .ingress
        .sessions(owner)
        .get(GetRequest {
            app_name: APP.into(),
            user_id: USER.into(),
            session_id: session.to_string(),
            num_recent_events: None,
            after: None,
        })
        .await
        .unwrap();
    let events = history.events().all();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].id, context.id);
    assert_eq!(
        events[0].content().unwrap().parts,
        context.content().unwrap().parts
    );
    assert_eq!(
        events[1].provider_metadata[crate::store::agent::INPUT_TURN],
        owner.to_string()
    );
    assert_eq!(
        events[1].content().unwrap().parts[0].text(),
        Some("Original user text")
    );
    node.ingress.agents.finish(owner);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn appends_changed_context_after_existing_input() {
    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let (session, owner) = running(&node).await;
    let batch = Batch {
        entries: vec![Entry {
            turn: owner,
            references: vec![Reference {
                target: sailry_protocol::conversation::reference::Target::Plugin("fixture".into()),
                label: "Fixture".into(),
            }],
            content: Content::new("user")
                .with_text("Selected references")
                .with_text("Original user text"),
            capability: None,
        }],
    };
    batch
        .append(&node.ingress, session, owner, "fixture", "user", None, None)
        .await
        .unwrap();
    let mut context = Event::new("context-fixture");
    context.author = "system".into();
    context
        .provider_metadata
        .insert(prompt::session::METADATA.into(), "context".into());
    context.set_content(Content::new("user").with_text("Changed runtime context"));
    context.actions.state_delta.insert(
        prompt::session::POLICY.into(),
        serde_json::json!("Changed policy"),
    );
    batch
        .append(
            &node.ingress,
            session,
            owner,
            "fixture",
            "user",
            None,
            Some(context.clone()),
        )
        .await
        .unwrap();
    let mut retried = context.clone();
    retried.id = "retried-context".into();
    batch
        .append(
            &node.ingress,
            session,
            owner,
            "fixture",
            "user",
            None,
            Some(retried),
        )
        .await
        .unwrap();
    let history = node
        .ingress
        .sessions(owner)
        .get(GetRequest {
            app_name: APP.into(),
            user_id: USER.into(),
            session_id: session.to_string(),
            num_recent_events: None,
            after: None,
        })
        .await
        .unwrap();
    let events = history.events().all();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].id, context.id);
    assert_eq!(
        serde_json::to_value(events[1].content()).unwrap(),
        serde_json::to_value(context.content()).unwrap()
    );
    assert!(
        !events[0]
            .provider_metadata
            .contains_key(crate::store::agent::CAPABILITY_PART)
    );
    assert_eq!(
        events[0].provider_metadata[crate::store::agent::INPUT_PART],
        "1"
    );
    let client = Client::new(node.local());
    let Output::Conversation(history) = client
        .execute(client.prepare(Command::ReadConversation {
            session,
            before: None,
            limit: 20,
        }))
        .await
        .unwrap()
    else {
        panic!("conversation history expected")
    };
    assert!(history.page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part, sailry_protocol::conversation::Part::Text(text) if text == "Original user text")));
    node.ingress.agents.finish(owner);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn resumes_partially_saved_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let (session, owner) = running(&node).await;
    let mut batch = Batch {
        entries: vec![Entry {
            turn: owner,
            references: vec![],
            content: Content::new("user").with_text("Original user text"),
            capability: None,
        }],
    };
    let mut context = Event::new("context-fixture");
    context.author = "system".into();
    context
        .provider_metadata
        .insert(prompt::session::METADATA.into(), "context".into());
    context.set_content(Content::new("user").with_text("Changed runtime context"));
    context.actions.state_delta.insert(
        prompt::session::POLICY.into(),
        serde_json::json!("Changed policy"),
    );
    batch
        .append(
            &node.ingress,
            session,
            owner,
            "fixture",
            "user",
            None,
            Some(context.clone()),
        )
        .await
        .unwrap();
    let client = Client::new(node.local());
    let Output::QueuedTurn(next) = client
        .execute(client.prepare(Command::QueueTurn {
            session,
            expected_revision: 1,
            message: "Remaining input".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("queued input expected")
    };
    batch.entries.push(Entry {
        turn: next.id,
        references: vec![],
        content: Content::new("user").with_text("Remaining input"),
        capability: None,
    });
    let original_context = context.id.clone();
    context.id = "retried-context".into();
    batch
        .append(
            &node.ingress,
            session,
            owner,
            "fixture",
            "user",
            None,
            Some(context),
        )
        .await
        .unwrap();
    let history = node
        .ingress
        .sessions(owner)
        .get(GetRequest {
            app_name: APP.into(),
            user_id: USER.into(),
            session_id: session.to_string(),
            num_recent_events: None,
            after: None,
        })
        .await
        .unwrap();
    let events = history.events().all();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].id, original_context);
    assert_eq!(
        events[1].provider_metadata[crate::store::agent::INPUT_TURN],
        owner.to_string()
    );
    assert_eq!(
        events[2].provider_metadata[crate::store::agent::INPUT_TURN],
        next.id.to_string()
    );
    assert_eq!(
        events[2].content().unwrap().parts[0].text(),
        Some("Remaining input")
    );
    node.ingress.agents.finish(owner);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn stamps_exact_capability_position() {
    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let (session, owner) = running(&node).await;
    let batch = Batch {
        entries: vec![Entry {
            turn: owner,
            references: vec![],
            content: Content::new("user")
                .with_text("Original user text")
                .with_text("Selected capability")
                .with_text("Following user data"),
            capability: Some(1),
        }],
    };
    batch
        .append(&node.ingress, session, owner, "fixture", "user", None, None)
        .await
        .unwrap();
    let history = node
        .ingress
        .sessions(owner)
        .get(GetRequest {
            app_name: APP.into(),
            user_id: USER.into(),
            session_id: session.to_string(),
            num_recent_events: None,
            after: None,
        })
        .await
        .unwrap();
    let events = history.events().all();
    assert_eq!(
        events[0].provider_metadata[crate::store::agent::CAPABILITY_PART],
        "1"
    );
    let client = Client::new(node.local());
    let Output::Conversation(history) = client
        .execute(client.prepare(Command::ReadConversation {
            session,
            before: None,
            limit: 20,
        }))
        .await
        .unwrap()
    else {
        panic!("conversation history expected")
    };
    let parts: Vec<_> = history
        .page
        .entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .collect();
    assert!(parts.iter().any(|part| matches!(part, sailry_protocol::conversation::Part::Text(text) if text == "Original user text")));
    assert!(parts.iter().any(|part| matches!(part, sailry_protocol::conversation::Part::Text(text) if text == "Following user data")));
    assert!(!parts.iter().any(|part| matches!(part, sailry_protocol::conversation::Part::Text(text) if text == "Selected capability")));
    node.ingress.agents.finish(owner);
    node.shutdown().await.unwrap();
}
