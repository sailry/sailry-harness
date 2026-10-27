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

#[tokio::test]
async fn reads_existing_event_bytes_without_rewriting() {
    let mut fixture = fixture::Fixture::new().await;
    fixture::files(&fixture.database, fixture._root.path()).await;
    let mut event = AdkEvent::new("recorded-invocation");
    event.author = "assistant".into();
    event.set_content(adk_core::Content::new("model").with_text("Recorded answer"));
    event.llm_response.usage_metadata = Some(adk_core::UsageMetadata {
        prompt_token_count: 11,
        candidates_token_count: 3,
        total_token_count: 14,
        ..Default::default()
    });
    let (decoded, body) = recorded(&event);
    let turn = fixture.append([decoded]).await;
    fixture
        .database
        .connection
        .execute(
            "UPDATE agent_events SET body=?1 WHERE turn=?2 AND id=?3",
            params![body.as_slice(), turn.to_string(), event.id.as_str()],
        )
        .await
        .unwrap();
    let page = fixture.read(None, 100).await.page;
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.id == event.id)
        .unwrap();
    assert_eq!(entry.parts, [Part::Text("Recorded answer".into())]);
    let stored: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT body FROM agent_events WHERE turn=?1 AND id=?2")
        .await
        .unwrap()
        .query_row(params![turn.to_string(), event.id.as_str()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert_eq!(stored, body);

    let mut call = AdkEvent::new("recorded-exchange");
    call.author = "assistant".into();
    call.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![adk_core::Part::FunctionCall {
            id: Some("recorded-call".into()),
            name: "read_file".into(),
            args: serde_json::json!({"path":"recorded.txt"}),
            thought_signature: None,
        }],
    });
    let mut result = AdkEvent::new("recorded-exchange");
    result.author = "tool".into();
    result.set_content(adk_core::Content {
        role: "user".into(),
        parts: vec![adk_core::Part::FunctionResponse {
            id: Some("recorded-call".into()),
            function_response: adk_core::FunctionResponseData::new(
                "read_file",
                serde_json::json!({"content":"Recorded tool result"}),
            ),
            annotations: None,
        }],
    });
    result
        .actions
        .state_delta
        .insert("recorded".into(), serde_json::json!(7));
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
    let (summary, _) = recorded(&summary);
    // Archived handoff metadata keeps its original role and bytes.
    let mut archived = summary.clone();
    archived
        .actions
        .compaction
        .as_mut()
        .unwrap()
        .compacted_content
        .role = "model".into();
    let (_, summary_body) = recorded(&archived);
    let compacted = fixture.append([summary.clone()]).await;
    let originals = [
        (turn, &event.id, &body),
        (exchange, &call.id, &call_body),
        (exchange, &result.id, &result_body),
        (compacted, &summary.id, &summary_body),
    ];
    for (turn, id, body) in originals {
        fixture
            .database
            .connection
            .execute(
                "UPDATE agent_events SET body=?1 WHERE turn=?2 AND id=?3",
                params![body.as_slice(), turn.to_string(), id.as_str()],
            )
            .await
            .unwrap();
    }
    let captured = captures(&fixture).await;
    let packages: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT plugins FROM turns WHERE id=?1")
        .await
        .unwrap()
        .query_row([turn.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert!(
        serde_json::from_slice::<serde_json::Value>(&packages)
            .unwrap()
            .to_string()
            .contains("files")
    );
    let original_page = fixture.read(None, 100).await.page;
    let context = sessions::get(
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
    assert_eq!(context.len(), 1);
    assert_eq!(context[0].id, summary.id);
    assert_eq!(
        serde_json::to_value(context[0].content()).unwrap(),
        serde_json::to_value(Some(
            &adk_core::Content::new("user").with_text("Recorded summary")
        ))
        .unwrap()
    );
    assert_eq!(
        history::state(&fixture.database.connection, fixture.session)
            .await
            .unwrap()["recorded"],
        7
    );
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(fixture.read(None, 100).await.page, original_page);
    let reloaded = sessions::get(
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
    assert_eq!(
        serde_json::to_value(&reloaded).unwrap(),
        serde_json::to_value(&context).unwrap()
    );
    assert_eq!(captures(&fixture).await, captured);
    for (turn, id, body) in originals {
        let stored: Vec<u8> = fixture
            .database
            .connection
            .prepare("SELECT body FROM agent_events WHERE turn=?1 AND id=?2")
            .await
            .unwrap()
            .query_row(params![turn.to_string(), id.as_str()])
            .await
            .and_then(|row| row.get(0))
            .unwrap();
        assert_eq!(&stored, body);
    }

    let request = serde_json::json!({"toolName":"recorded-tool", "functionCallId":"recorded-call", "args":{"path":"recorded.txt"}});
    let decoded: adk_core::ToolConfirmationRequest =
        serde_json::from_value(request.clone()).unwrap();
    assert!(decoded.acting.is_none());
    assert!(decoded.origin.is_none());
    assert_eq!(serde_json::to_value(decoded).unwrap(), request);
}
