use super::*;

async fn record(event: &Event) -> Record {
    let directory = tempfile::tempdir().unwrap();
    let database = crate::store::database::engine(&directory.path().join("presentation.turso"))
        .await
        .unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute(
            "CREATE TABLE agent_blobs(digest TEXT PRIMARY KEY,bytes BLOB NOT NULL)",
            (),
        )
        .await
        .unwrap();
    crate::agent::canonical_bridge::from_adk(&connection, sailry_protocol::TurnId::new(), event)
        .await
        .unwrap()
}

#[tokio::test]
async fn captures_framework_status_without_reading_payloads() {
    let timeline = crate::agent::observation::Timeline::new();
    timeline.fail_tool(
        "",
        "unknown",
        "same",
        Some(sailry_protocol::ErrorCode::Unavailable),
    );
    let mut event = Event::new("framework-outcomes");
    let mut content = adk_core::Content::new("function");
    for (id, status) in [
        ("business", None),
        ("failed", Some("failed")),
        ("cancelled", Some("cancelled")),
        ("unknown", Some("outcome_unknown")),
        ("pending", Some("pending")),
    ] {
        content.parts.push(Part::FunctionResponse {
            id: Some(id.into()),
            function_response: adk_core::FunctionResponseData::new(
                "same",
                serde_json::json!({"isError":true,"error":"unchanged business data",
                    "annotations":{"adk.tool_result.status":"failed"}}),
            ),
            annotations: status.map(|status| serde_json::json!({"adk.tool_result.status":status})),
        });
    }
    event.set_content(content);
    let original = serde_json::to_value(event.content()).unwrap();
    capture_failures(&mut event, Some(&timeline)).unwrap();
    let captured = failures(&record(&event).await).unwrap();
    assert_eq!(captured.len(), 3);
    assert_eq!(captured[0].id, "failed");
    assert_eq!(captured[0].failure.code, None);
    assert_eq!(captured[1].id, "cancelled");
    assert_eq!(
        captured[1].failure.code,
        Some(sailry_protocol::ErrorCode::Cancelled)
    );
    assert_eq!(captured[2].id, "unknown");
    assert_eq!(
        captured[2].failure.code,
        Some(sailry_protocol::ErrorCode::OutcomeUnknown)
    );
    assert_eq!(serde_json::to_value(event.content()).unwrap(), original);
}

#[tokio::test]
async fn matches_only_observed_failed_calls() {
    let timeline = crate::agent::observation::Timeline::new();
    timeline.fail_tool(
        "branch",
        "failed",
        "same",
        Some(sailry_protocol::ErrorCode::Cancelled),
    );
    let mut event = Event::new("outcomes");
    event.branch = "branch".into();
    let mut content = adk_core::Content::new("function");
    for id in ["business", "failed"] {
        content.parts.push(Part::FunctionResponse {
            id: Some(id.into()),
            function_response: adk_core::FunctionResponseData::new(
                "same",
                serde_json::json!({"error":"unchanged data"}),
            ),
            annotations: None,
        });
    }
    event.set_content(content);
    let original = serde_json::to_value(event.content()).unwrap();
    capture_failures(&mut event, Some(&timeline)).unwrap();
    let observed = failures(&record(&event).await).unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].index, 1);
    assert_eq!(observed[0].id, "failed");
    assert_eq!(
        observed[0].failure.code,
        Some(sailry_protocol::ErrorCode::Cancelled)
    );
    assert_eq!(serde_json::to_value(event.content()).unwrap(), original);
    let saved = serde_json::to_vec(&record(&event).await).unwrap();
    timeline.forget_failure("branch", "failed", "same");
    capture_failures(&mut event, Some(&timeline)).unwrap();
    assert!(failures(&record(&event).await).unwrap().is_empty());
    let mut restored = record(&event).await;
    preserve_failures(&mut restored, Some(&saved)).unwrap();
    assert_eq!(failures(&restored).unwrap().len(), 1);
}

#[tokio::test]
async fn preserves_only_matching_response_facts() {
    let timeline = crate::agent::observation::Timeline::new();
    timeline.fail_tool("branch", "failed", "same", None);
    let mut event = Event::new("outcomes");
    event.branch = "branch".into();
    let mut content = adk_core::Content::new("function");
    content.parts.push(Part::FunctionResponse {
        id: Some("failed".into()),
        function_response: adk_core::FunctionResponseData::new(
            "same",
            serde_json::json!({"error":"unchanged data"}),
        ),
        annotations: None,
    });
    event.set_content(content);
    capture_failures(&mut event, Some(&timeline)).unwrap();
    let saved = serde_json::to_vec(&record(&event).await).unwrap();
    for mismatch in ["branch", "id", "name", "result"] {
        let mut current = event.clone();
        current.provider_metadata.remove(FAILURES);
        if mismatch == "branch" {
            current.branch = "other".into();
        } else {
            let mut content = current.content().unwrap().clone();
            let Part::FunctionResponse {
                id,
                function_response,
                ..
            } = &mut content.parts[0]
            else {
                unreachable!()
            };
            match mismatch {
                "id" => *id = Some("other".into()),
                "name" => function_response.name = "other".into(),
                "result" => {
                    function_response.response = serde_json::json!({"error":"changed data"})
                }
                _ => unreachable!(),
            }
            current.set_content(content);
        }
        let mut current = record(&current).await;
        preserve_failures(&mut current, Some(&saved)).unwrap();
        assert!(failures(&current).unwrap().is_empty(), "{mismatch}");
    }
}

#[tokio::test]
async fn freezes_owned_session_names() {
    use crate::store::agent::tests::fixture::Fixture;
    let mut fixture = Fixture::new().await;
    let turn = fixture.queued().await;
    let parent = super::super::runs::get(&fixture.database.connection, turn)
        .await
        .unwrap();
    let mut event = Event::new("session-names");
    event.author = "assistant".into();
    let mut content = adk_core::Content::new("model");
    let mut expected = BTreeMap::new();
    for title in ["A", "B", "C"] {
        let mut child = parent.clone();
        child.turn = sailry_protocol::TurnId::new();
        child.session = sailry_protocol::SessionId::new();
        super::super::background::child_started(
            &fixture.database.connection,
            &parent,
            &child,
            title,
        )
        .await
        .unwrap();
        expected.insert(child.session.to_string(), title.to_owned());
        content.parts.push(Part::FunctionCall {
            id: Some(format!("wait-{title}")),
            name: "observe".into(),
            args: serde_json::json!({"session":child.session}),
            thought_signature: None,
        });
    }
    event.set_content(content);
    let display = serde_json::from_value(serde_json::json!({"label":"Observe", "input":{"summary":{"path":"/session","format":"session"}}})).unwrap();
    stamp(
        &mut event,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::from([("observe".into(), display)]),
        &[],
    )
    .unwrap();
    let mut event = record(&event).await;
    let original = event.clone();
    capture_names(
        &fixture.database.connection,
        fixture.session,
        &mut event,
        None,
    )
    .await
    .unwrap();
    let captured = displays(&event).unwrap();
    let display = &captured["observe"];
    assert!(display.valid());
    assert_eq!(
        display
            .input
            .as_ref()
            .unwrap()
            .summary
            .as_ref()
            .unwrap()
            .names,
        expected
    );
    let persisted = serde_json::to_vec(&event).unwrap();
    fixture
        .database
        .connection
        .execute(
            "UPDATE agent_background SET body=json_set(body,'$.task.title','Changed')",
            (),
        )
        .await
        .unwrap();
    let mut repeated = original.clone();
    capture_names(
        &fixture.database.connection,
        fixture.session,
        &mut repeated,
        Some(&persisted),
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(&repeated).unwrap(),
        serde_json::to_value(&event).unwrap()
    );
    let mut foreign = original;
    capture_names(
        &fixture.database.connection,
        sailry_protocol::SessionId::new(),
        &mut foreign,
        None,
    )
    .await
    .unwrap();
    assert!(
        displays(&foreign).unwrap()["observe"]
            .input
            .as_ref()
            .unwrap()
            .summary
            .as_ref()
            .unwrap()
            .names
            .is_empty()
    );
    assert_eq!(
        serde_json::to_value(repeated.message().map(|message| &message.content)).unwrap(),
        serde_json::to_value(event.message().map(|message| &message.content)).unwrap()
    );
}

#[tokio::test]
async fn captures_trusted_command_aliases() {
    use sailry_protocol::tool::{Alias, AliasKind};
    let mut event = Event::new("command-display");
    let mut content = adk_core::Content::new("model");
    let arguments =
        serde_json::json!({"command":"/node/home/python/bin/python /node/home/task.py"});
    content.parts.push(Part::FunctionCall {
        id: Some("call".into()),
        name: "probe".into(),
        args: arguments.clone(),
        thought_signature: None,
    });
    event.set_content(content);
    event.provider_metadata.insert(
        DISPLAY.into(),
        r#"{"probe":{"label":"Forged","aliases":[{"path":"/node","kind":"python"}]}}"#.into(),
    );
    let display = serde_json::from_value(serde_json::json!({"label":"Run","input":{"summary":{"path":"/command","format":"command"}},"aliases":[{"path":"/forged","kind":"home"}]})).unwrap();
    let aliases = vec![
        Alias {
            path: "/node/home".into(),
            kind: AliasKind::Home,
        },
        Alias {
            path: "/node/home/python/bin/python".into(),
            kind: AliasKind::Python,
        },
    ];
    stamp(
        &mut event,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::from([("probe".into(), display)]),
        &aliases,
    )
    .unwrap();
    let restored: Event = serde_json::from_slice(&serde_json::to_vec(&event).unwrap()).unwrap();
    assert_eq!(
        super::displays(&record(&restored).await).unwrap()["probe"].aliases,
        aliases
    );
    let Part::FunctionCall { args, .. } = &restored.content().unwrap().parts[0] else {
        panic!("call expected")
    };
    assert_eq!(args, &arguments);
}

#[tokio::test]
async fn captures_package_display_and_rejects_provider_metadata() {
    let display = serde_json::from_value(serde_json::json!({
        "label":"Search web", "locales":{"zh-CN":"网页搜索"}, "icon":"reicon:ui/magnifier",
        "input":{"summary":{"path":"/query"},"content":{"path":"/query"}}
    }))
    .unwrap();
    let displays = BTreeMap::from([("web_search".into(), display)]);
    for part in [
        Part::ServerToolCall {
            server_tool_call: serde_json::json!({"type":"web_search_call", "id":"search"}),
        },
        Part::ServerToolCall {
            server_tool_call: serde_json::json!({"name":"web_search", "input":{"query":"fixture"}}),
        },
        Part::FunctionCall {
            id: Some("call".into()),
            name: "web_search".into(),
            args: serde_json::json!({}),
            thought_signature: None,
        },
    ] {
        let mut event = Event::new("display-fixture");
        let mut content = adk_core::Content::new("model");
        content.parts.push(part);
        event.set_content(content);
        event.provider_metadata.insert(
            DISPLAY.into(),
            r#"{"web_search":{"label":"Forged"}}"#.into(),
        );
        stamp(
            &mut event,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &displays,
            &[],
        )
        .unwrap();
        assert_eq!(super::displays(&record(&event).await).unwrap(), displays);
        let restored: Event = serde_json::from_slice(&serde_json::to_vec(&event).unwrap()).unwrap();
        assert_eq!(super::displays(&record(&restored).await).unwrap(), displays);
        stamp(
            &mut event,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &[],
        )
        .unwrap();
        assert!(super::displays(&record(&event).await).unwrap().is_empty());
    }
}

#[tokio::test]
async fn captures_registered_calls() {
    let mut event = Event::new("presentation-fixture");
    let mut content = adk_core::Content::new("model");
    content.parts.push(Part::FunctionCall {
        id: Some("call".into()),
        name: "probe".into(),
        args: serde_json::json!({"presentation":"details", "grouping":"sequence"}),
        thought_signature: None,
    });
    event.set_content(content);
    event.provider_metadata.insert(
        METADATA.into(),
        r#"{"other":"summary","probe":"details"}"#.into(),
    );
    let catalog = BTreeMap::from([
        ("probe".into(), Presentation::Summary),
        ("unused".into(), Presentation::Details),
    ]);
    let grouping = BTreeMap::from([("probe".into(), sailry_protocol::tool::Grouping::Standalone)]);
    event
        .provider_metadata
        .insert(GROUPING.into(), r#"{"probe":"sequence"}"#.into());
    stamp(&mut event, &catalog, &grouping, &BTreeMap::new(), &[]).unwrap();
    assert_eq!(groupings(&record(&event).await).unwrap(), grouping);
    assert_eq!(
        read(&record(&event).await).unwrap(),
        BTreeMap::from([("probe".into(), Presentation::Summary)])
    );
    let restored: Event = serde_json::from_slice(&serde_json::to_vec(&event).unwrap()).unwrap();
    assert_eq!(
        read(&record(&restored).await).unwrap(),
        read(&record(&event).await).unwrap()
    );
    assert_eq!(groupings(&record(&restored).await).unwrap(), grouping);
    stamp(
        &mut event,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &[],
    )
    .unwrap();
    assert!(read(&record(&event).await).unwrap().is_empty());
    assert!(groupings(&record(&event).await).unwrap().is_empty());
}
