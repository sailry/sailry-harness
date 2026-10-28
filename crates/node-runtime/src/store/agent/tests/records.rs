use super::*;

fn recorded(event: &AdkEvent) -> (AdkEvent, Vec<u8>) {
    let mut value = serde_json::to_value(event).unwrap();
    let response = value.as_object_mut().unwrap();
    response.remove("model");
    response.remove("provider");
    if let Some(usage) = response
        .get_mut("usage_metadata")
        .and_then(serde_json::Value::as_object_mut)
    {
        usage.remove("builtin_tool_uses");
    }
    let actions = value["actions"].as_object_mut().unwrap();
    actions.remove("input_screened");
    actions.remove("workflow_positions");
    let body = serde_json::to_vec_pretty(&value).unwrap();
    let recorded: AdkEvent = serde_json::from_slice(&body).unwrap();
    assert_eq!(serde_json::to_value(&recorded).unwrap(), value);
    (recorded, body)
}

async fn captures(fixture: &fixture::Fixture) -> String {
    fixture
        .database
        .connection
        .prepare(
            "SELECT json_group_array(json_object('id',t.id,'plugins',hex(t.plugins),
         'roles',hex(t.roles),'instructions',hex(t.instructions),'revision',t.revision,
         'config',hex(r.config),'captured_roles',hex(r.roles)))
         FROM (SELECT * FROM turns ORDER BY id) t JOIN session_revisions r
         ON r.session=t.session AND r.revision=t.revision",
        )
        .await
        .unwrap()
        .query_row(())
        .await
        .and_then(|row| row.get(0))
        .unwrap()
}

#[tokio::test]
async fn projects_only_tagged_node_context() {
    use adk_core::Session as _;

    let mut fixture = fixture::Fixture::new().await;
    let text = "Node context\n\nPersonal instructions\nKeep the captured policy";
    let mut context = AdkEvent::new("captured-context");
    context.author = "system".into();
    context
        .provider_metadata
        .insert(crate::agent::CONTEXT_METADATA.into(), "context".into());
    context.actions.state_delta.insert(
        "sailry.context.personal".into(),
        serde_json::json!("Keep the captured policy"),
    );
    context.set_content(adk_core::Content::new("user").with_text(text));
    let mut untagged = AdkEvent::new("untagged-context");
    untagged.author = "system".into();
    untagged.set_content(adk_core::Content::new("user").with_text("Other system content"));
    let mut other = context.clone();
    other.id = "other-agent-context".into();
    other.author = "other".into();
    let turn = fixture.append([context.clone(), untagged, other]).await;
    let request = GetRequest {
        app_name: APP.into(),
        user_id: USER.into(),
        session_id: fixture.session.to_string(),
        num_recent_events: None,
        after: None,
    };
    let canonical = sessions::get(&fixture.database.connection, turn, request.clone())
        .await
        .unwrap();
    let originals = serde_json::to_value(canonical.events().all()).unwrap();
    assert_eq!(originals[0]["author"], "system");
    let state = canonical.state().all();
    let updated = canonical.last_update_time();
    let projected = canonical.for_model();
    assert_eq!(projected.events().at(0).unwrap().author, "user");
    assert_eq!(projected.events().at(1).unwrap().author, "system");
    assert_eq!(projected.events().at(2).unwrap().author, "other");
    assert_eq!(projected.state().all(), state);
    assert_eq!(projected.last_update_time(), updated);
    assert_eq!(projected.events().at(0).unwrap().id, context.id);
    assert!(
        !projected
            .events()
            .at(0)
            .unwrap()
            .provider_metadata
            .contains_key(sessions::INPUT_TURN)
    );
    let runner = adk_runner::MutableSession::new(Arc::new(projected));
    let contents = runner.conversation_history_with_context("assistant", "");
    assert_eq!(contents[0].role, "user");
    assert_eq!(contents[0].parts[0].text(), Some(text));
    assert!(!adk_core::history::is_other_agent_context(&contents[0]));
    assert!(adk_core::history::is_other_agent_context(&contents[1]));
    assert!(adk_core::history::is_other_agent_context(&contents[2]));
    let reloaded = sessions::get(&fixture.database.connection, turn, request)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(reloaded.events().all()).unwrap(),
        originals
    );
    assert_eq!(reloaded.state().all(), state);
    assert!(
        fixture
            .read(None, 100)
            .await
            .page
            .entries
            .iter()
            .find(|entry| entry.id == context.id)
            .unwrap()
            .parts
            .is_empty()
    );
}

async fn body(db: &turso::Connection, id: &str) -> Vec<u8> {
    db.prepare("SELECT body FROM agent_events WHERE id=?1")
        .await
        .unwrap()
        .query_row([id])
        .await
        .unwrap()
        .get(0)
        .unwrap()
}

async fn legacy(db: &turso::Connection, records: &[(TurnId, AdkEvent, Vec<u8>)]) {
    for (turn, event, body) in records {
        db.execute(
            "UPDATE agent_events SET body=?1 WHERE turn=?2 AND id=?3",
            params![body.as_slice(), turn.to_string(), event.id.as_str()],
        )
        .await
        .unwrap();
    }
    db.execute_batch("DROP INDEX agent_usage; DROP INDEX agent_usage_time; DROP TABLE agent_blobs")
        .await
        .unwrap();
    db.execute_batch(include_str!("../../database/schema/upgrades/usage.sql"))
        .await
        .unwrap();
}

#[tokio::test]
async fn upgrades_history_with_private_backup() {
    use crate::agent::canonical::{BlockKind, Record};
    let mut fixture = fixture::Fixture::new().await;
    fixture::files(&fixture.database, fixture._root.path()).await;
    let mut answer = AdkEvent::new("recorded-invocation");
    answer.author = "assistant".into();
    answer.branch = "recorded-scope".into();
    answer.set_content(adk_core::Content::new("model").with_text("Recorded answer"));
    answer.llm_response.usage_metadata = Some(adk_core::UsageMetadata {
        prompt_token_count: 11,
        candidates_token_count: 3,
        total_token_count: 14,
        ..Default::default()
    });
    let (answer, answer_body) = recorded(&answer);
    let first = fixture.append([answer.clone()]).await;
    let mut call = AdkEvent::new("recorded-exchange");
    call.author = "assistant".into();
    call.branch = "recorded-scope".into();
    call.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![
            adk_core::Part::Text {
                text: "Before the call".into(),
            },
            adk_core::Part::FunctionCall {
                id: Some("recorded-call".into()),
                name: "read_file".into(),
                args: serde_json::json!({"path":"recorded.txt"}),
                thought_signature: Some("recorded-signature".into()),
            },
        ],
    });
    let original = serde_json::json!({
        "content":"Recorded tool result",
        "image":{"mimeType":"image/png","data":"AAEC/f7/"}
    });
    let bytes = vec![0, 1, 2, 253, 254, 255];
    let mut result = AdkEvent::new("recorded-exchange");
    result.author = "tool".into();
    result.branch = call.branch.clone();
    result.set_content(adk_core::Content {
        role: "user".into(),
        parts: vec![
            adk_core::Part::Text {
                text: "Before the result".into(),
            },
            adk_core::Part::FunctionResponse {
                id: Some("recorded-call".into()),
                function_response: adk_core::FunctionResponseData::with_inline_data(
                    "read_file",
                    original.clone(),
                    vec![adk_core::InlineDataPart {
                        mime_type: "image/png".into(),
                        data: bytes.clone(),
                        uri: None,
                        annotations: None,
                    }],
                ),
                annotations: None,
            },
        ],
    });
    result.actions.state_delta.insert(
        "sailry.context.personal".into(),
        serde_json::json!("Keep the captured policy"),
    );
    let (call, call_body) = recorded(&call);
    let (result, result_body) = recorded(&result);
    let exchange = fixture.append([call.clone(), result.clone()]).await;
    let mut summary = AdkEvent::new("recorded-summary");
    summary.author = "system".into();
    summary.provider_metadata.insert(
        super::super::compaction::END_EVENT.into(),
        result.id.clone(),
    );
    summary.actions.compaction = Some(adk_core::EventCompaction {
        start_timestamp: call.timestamp,
        end_timestamp: result.timestamp,
        compacted_content: adk_core::Content::new("user").with_text("Recorded summary"),
    });
    let compacted = fixture.append([summary.clone()]).await;
    // Old handoffs could carry the model role; startup normalizes their model projection.
    summary
        .actions
        .compaction
        .as_mut()
        .unwrap()
        .compacted_content
        .role = "model".into();
    let (summary, summary_body) = recorded(&summary);
    let originals = vec![
        (first, answer.clone(), answer_body),
        (exchange, call.clone(), call_body),
        (exchange, result.clone(), result_body),
        (compacted, summary.clone(), summary_body),
    ];
    let queue = queue::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let session = fixture.session;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetQueuePaused {
            session,
            expected_revision: queue.revision,
            paused: true,
        },
    )
    .await;
    let captured = captures(&fixture).await;
    let projected = fixture.read(None, 100).await;
    let entry = projected
        .page
        .entries
        .iter()
        .find(|entry| entry.id == result.id)
        .unwrap();
    let Part::ToolResult {
        id,
        summary: detail,
        images,
        ..
    } = &entry.parts[1]
    else {
        panic!("recorded tool result expected")
    };
    assert_eq!(images[0].part, 1);
    let image = images[0].clone();
    let reference = sailry_protocol::tool::OutputReference {
        call: id.clone().unwrap(),
        revision: blake3::hash(original.to_string().as_bytes())
            .to_hex()
            .to_string(),
    };
    let detail_revision = detail.revision.clone();
    legacy(&fixture.database.connection, &originals).await;
    let path = fixture.database.path.clone();
    let fixture::Fixture {
        database,
        events,
        session,
        _root,
    } = fixture;
    database.close().await.unwrap();
    let database = Database::open(&path, NodeId([33; 32]), None).await.unwrap();
    let mut fixture = fixture::Fixture {
        database,
        events,
        session,
        _root,
    };
    assert_eq!(fixture.read(None, 100).await, projected);
    assert_eq!(captures(&fixture).await, captured);
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, fixture.session, &image)
            .await
            .unwrap(),
        bytes
    );
    let record: Record =
        serde_json::from_slice(&body(&fixture.database.connection, &result.id).await).unwrap();
    let BlockKind::ToolResponse(response) = &record.message().unwrap().content[1].kind else {
        panic!("canonical result expected")
    };
    assert_eq!(record.provenance.author, result.author);
    assert_eq!(record.provenance.branch, result.branch);
    assert_eq!(response.revision, reference.revision);
    assert!(response.result.value["image"]["data"].is_null());
    assert_eq!(
        super::super::blobs::hydrate(&fixture.database.connection, &response.result)
            .await
            .unwrap(),
        original
    );
    let restored = sessions::get(
        &fixture.database.connection,
        compacted,
        GetRequest {
            app_name: APP.into(),
            user_id: USER.into(),
            session_id: fixture.session.to_string(),
            num_recent_events: None,
            after: None,
        },
    )
    .await
    .unwrap()
    .events()
    .all();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id, summary.id);
    assert_eq!(restored[0].content().unwrap().role, "user");
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(fixture.read(None, 100).await, projected);
    let current = fixture.read(None, 100).await;
    let entry = current
        .page
        .entries
        .iter()
        .find(|entry| entry.id == result.id)
        .unwrap();
    assert!(
        matches!(&entry.parts[1], Part::ToolResult { summary, .. } if summary.revision == detail_revision)
    );
    let backups: Vec<_> = std::fs::read_dir(fixture._root.path().join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("node.turso"))
        .collect();
    assert_eq!(backups.len(), 1);
    let backup = crate::store::database::engine(&backups[0]).await.unwrap();
    let connection = crate::store::database::connect(&backup).await.unwrap();
    for (_, event, original) in &originals {
        assert_eq!(body(&connection, &event.id).await, *original);
    }
    let has_assets: bool = connection
        .prepare("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='agent_blobs')")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert!(!has_assets);
    drop(connection);
    drop(backup);
    fixture.database.close().await.unwrap();
    let database = Database::open(&path, NodeId([33; 32]), None).await.unwrap();
    let fixture = fixture::Fixture {
        database,
        ..fixture
    };
    assert_eq!(fixture.read(None, 100).await, projected);
    assert_eq!(
        std::fs::read_dir(fixture._root.path().join("backups"))
            .unwrap()
            .count(),
        1
    );
}

#[tokio::test]
async fn rejects_invalid_history_without_writes() {
    let mut fixture = fixture::Fixture::new().await;
    let mut first = AdkEvent::new("first");
    first.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![adk_core::Part::InlineData {
            mime_type: "image/png".into(),
            data: vec![1, 2, 3],
            uri: None,
            annotations: None,
        }],
    });
    let mut second = AdkEvent::new("second");
    second.set_content(adk_core::Content::new("model").with_text("Later record"));
    let turn = fixture.append([first.clone(), second.clone()]).await;
    let (_, first_body) = recorded(&first);
    let invalid = br#"{"id":"second","partial":true}"#.to_vec();
    let originals = vec![
        (turn, first.clone(), first_body),
        (turn, second.clone(), invalid),
    ];
    legacy(&fixture.database.connection, &originals).await;
    let path = fixture.database.path.clone();
    let root = fixture._root.path().to_owned();
    fixture.database.close().await.unwrap();
    let error = match Database::open(&path, NodeId([33; 32]), None).await {
        Ok(_) => panic!("invalid history was accepted"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("cannot be upgraded (decode:"), "{error}");
    assert!(!root.join("backups").exists());
    let handle = crate::store::database::engine(&path).await.unwrap();
    let connection = crate::store::database::connect(&handle).await.unwrap();
    for (_, event, original) in &originals {
        assert_eq!(body(&connection, &event.id).await, *original);
    }
    let assets: bool = connection
        .prepare("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='agent_blobs')")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert!(!assets);
}
