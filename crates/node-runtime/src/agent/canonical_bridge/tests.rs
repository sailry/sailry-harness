use super::*;
use adk_core::{FunctionResponseData, Part, UsageMetadata};

async fn database() -> (turso::Database, Connection) {
    let database = turso::Builder::new_local(":memory:").build().await.unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute(
            "CREATE TABLE agent_blobs(digest TEXT PRIMARY KEY,bytes BLOB NOT NULL)",
            (),
        )
        .await
        .unwrap();
    (database, connection)
}

#[tokio::test]
async fn preserves_calls_and_binary_revisions() {
    let (_database, db) = database().await;
    let turn = TurnId::new();
    let mut event = Event::new(turn.to_string());
    event.author = "read_file".into();
    event.branch = "worker.child".into();
    let raw = json!({"content":[{"type":"image","mimeType":"image/png","data":"AAECAw=="}],"unknown":{"exact":[null,false,3,"four"]}});
    event.set_content(Content {
        role: "function".into(),
        parts: vec![
            Part::FunctionCall {
                id: Some("call-preserved".into()),
                name: "read_file".into(),
                args: json!({"path":"src/lib.rs"}),
                thought_signature: Some("signature".into()),
            },
            Part::FunctionResponse {
                id: Some("call-preserved".into()),
                annotations: Some(json!({"audience":["user"]})),
                function_response: FunctionResponseData {
                    name: "read_file".into(),
                    response: raw.clone(),
                    inline_data: Vec::new(),
                    file_data: Vec::new(),
                },
            },
            Part::InlineData {
                mime_type: "image/png".into(),
                data: vec![7; 11 * 1024 * 1024],
                uri: Some("capture.png".into()),
                annotations: None,
            },
        ],
    });
    let record = from_adk(&db, turn, &event).await.unwrap();
    let message = record.message().unwrap();
    assert_eq!(message.part_count, 3);
    let BlockKind::ToolRequest(call) = &message.content[0].kind else {
        panic!("tool request expected")
    };
    assert_eq!(call.id.as_deref(), Some("call-preserved"));
    assert_eq!(call.signature.as_deref(), Some("signature"));
    let BlockKind::ToolResponse(result) = &message.content[1].kind else {
        panic!("tool response expected")
    };
    assert_eq!(
        result.revision,
        blake3::hash(&serde_json::to_vec(&raw).unwrap())
            .to_hex()
            .to_string()
    );
    assert!(result.result.value["content"][0]["data"].is_null());
    assert_eq!(result.result.assets.len(), 1);
    assert!(serde_json::to_vec(&record).unwrap().len() < 4096);
    let restored = to_adk(&db, &record).await.unwrap();
    assert_eq!(restored.id, event.id);
    assert_eq!(restored.author, event.author);
    assert_eq!(restored.branch, event.branch);
    assert_eq!(
        serde_json::to_value(restored.content()).unwrap(),
        serde_json::to_value(event.content()).unwrap(),
    );
}

#[tokio::test]
async fn keeps_slot_bounds_without_opaque_replay() {
    let (_database, db) = database().await;
    let mut event = Event::new("slots");
    event.author = "user".into();
    let parts: Content = serde_json::from_value(json!({"role":"user","parts":[
        {"configuration_update":{"effort":"low"}},
        {"text":"Task"},
        {"configuration_update":{"effort":"high"}}
    ]}))
    .unwrap();
    event.set_content(parts);
    event
        .provider_metadata
        .insert("sailry.context.notice_ids".into(), "[]".into());
    let record = from_adk(&db, TurnId::new(), &event).await.unwrap();
    let message = record.message().unwrap();
    assert_eq!(message.part_count, 3);
    assert_eq!(message.content.len(), 1);
    assert_eq!(message.content[0].index, 1);
    assert_eq!(record.metadata.input.as_ref().unwrap().notice_part, Some(1));
    assert!(
        !serde_json::to_string(&record)
            .unwrap()
            .contains("configuration_update")
    );
    assert_eq!(
        to_adk(&db, &record)
            .await
            .unwrap()
            .content()
            .unwrap()
            .parts
            .len(),
        3
    );
}

#[test]
fn keeps_context_and_telemetry_visibility() {
    let mut event = Event::new("context");
    event.author = "system".into();
    event
        .provider_metadata
        .insert("sailry_context_projection".into(), "{\"tokens\":42}".into());
    let metadata = metadata::capture(&event).unwrap();
    assert!(!metadata.runtime.unwrap().user_visible);
    event
        .provider_metadata
        .insert("sailry.context".into(), "context".into());
    event
        .actions
        .state_delta
        .insert("sailry.context.policy".into(), json!("Policy"));
    event.actions.state_delta.insert(
        "sailry.context.guide:original".into(),
        json!({"path":"AGENTS.md","revision":"revision","content":"Instructions"}),
    );
    event
        .actions
        .state_delta
        .insert("sailry.context.loaded_tools".into(), json!(["read_file"]));
    let captured = metadata::capture(&event).unwrap();
    assert!(captured.runtime.as_ref().unwrap().user_visible);
    assert_eq!(
        state(captured.context.as_ref().unwrap()),
        event.actions.state_delta
    );
    let mut restored = Event::new("context");
    metadata::restore(&mut restored, &captured, captured.context.as_ref()).unwrap();
    assert_eq!(restored.actions.state_delta, event.actions.state_delta);
}

#[test]
fn keeps_scoped_extension_state() {
    let mut event = Event::new("state");
    event.actions.state_delta.extend([
        ("saved".into(), json!({"value":1})),
        ("app:shared".into(), json!(2)),
        ("user:preference".into(), json!(null)),
        ("temp:transient".into(), json!(3)),
        ("sailry.context.policy".into(), json!("Policy")),
    ]);
    let captured = metadata::capture(&event).unwrap();
    let state = captured.state.as_ref().unwrap();
    assert_eq!(state.session, [("saved".into(), json!({"value":1}))].into());
    assert_eq!(state.application, [("shared".into(), json!(2))].into());
    assert_eq!(state.user, [("preference".into(), json!(null))].into());
    let mut restored = Event::new("state");
    metadata::restore(&mut restored, &captured, captured.context.as_ref()).unwrap();
    event.actions.state_delta.remove("temp:transient");
    assert_eq!(restored.actions.state_delta, event.actions.state_delta);
}

#[test]
fn keeps_unknown_usage_and_normalizes_warning() {
    let mut usage = UsageMetadata::default();
    usage.prompt_token_count = -1;
    usage.candidates_token_count = 12;
    let usage = response::usage(&usage);
    assert_eq!(usage.input, None);
    assert_eq!(usage.output, Some(12));
    assert_eq!(usage.cache_read, None);
    let mut event = Event::new("warning");
    event.author = "system".into();
    event.provider_metadata.insert(
        "sailry_operation_warning".into(),
        "context_summary_failed".into(),
    );
    event.llm_response.provider_metadata =
        Some(json!({"sailry_timing":{"elapsed_us":90,"first_token_us":null}}));
    let captured = metadata::capture(&event).unwrap();
    assert!(
        captured
            .compaction
            .as_ref()
            .unwrap()
            .failure
            .as_ref()
            .unwrap()
            .continued
    );
    assert_eq!(
        captured
            .execution
            .as_ref()
            .unwrap()
            .timing
            .as_ref()
            .unwrap()
            .first_token_us,
        None
    );
    let mut restored = Event::new("warning");
    metadata::restore(&mut restored, &captured, captured.context.as_ref()).unwrap();
    assert!(
        restored
            .provider_metadata
            .contains_key("sailry_compaction_failure")
    );
    assert!(
        !restored
            .provider_metadata
            .contains_key("sailry_operation_warning")
    );
}

#[tokio::test]
async fn keeps_summary_boundary_and_files() {
    let (_database, db) = database().await;
    let mut event = Event::new("summary");
    event.author = "system".into();
    event.actions.compaction = Some(adk_core::EventCompaction {
        start_timestamp: event.timestamp,
        end_timestamp: event.timestamp,
        compacted_content: Content::new("user").with_text("Handoff"),
    });
    event.provider_metadata.insert(
        "sailry.context.end_event".into(),
        "last-original-event".into(),
    );
    event.provider_metadata.insert(
        "sailry.compaction.files".into(),
        "{\"src/lib.rs\":true}".into(),
    );
    let record = from_adk(&db, TurnId::new(), &event).await.unwrap();
    let restored = to_adk(&db, &record).await.unwrap();
    assert_eq!(
        restored.provider_metadata["sailry.context.end_event"],
        "last-original-event"
    );
    assert_eq!(
        restored.provider_metadata["sailry.compaction.files"],
        event.provider_metadata["sailry.compaction.files"]
    );
    assert_eq!(
        restored.actions.compaction.unwrap().compacted_content.parts,
        event.actions.compaction.unwrap().compacted_content.parts
    );
}

#[tokio::test]
async fn retains_public_tools_and_redacted_order() {
    let (_database, db) = database().await;
    let owned = |part| Part::ProviderData {
        provider_data: adk_core::ProviderData {
            origin: None,
            parts: Box::new([part]),
        },
    };
    let mut event = Event::new("native");
    event.author = "assistant".into();
    event.set_content(Content { role: "model".into(), parts: vec![
        owned(Part::FunctionCall { id: Some("call".into()), name: "read_file".into(), args: json!({"path":"a.txt"}), thought_signature: None }),
        Part::Text { text: "Answer".into() },
        owned(Part::ServerToolResponse { server_tool_response: json!({"type":"anthropic_message","content":[
            {"type":"redacted_thinking","data":"first"},
            {"type":"tool_use","id":"call","name":"read_file","input":{"path":"a.txt"}},
            {"type":"redacted_thinking","data":"second"},
            {"type":"text","text":"Answer"}
        ]}) }),
        owned(Part::ServerToolCall { server_tool_call: json!({"type":"web_search_call","id":"search","action":{"type":"search","query":"public query"}}) }),
        owned(Part::ServerToolCall { server_tool_call: json!({"type":"message","phase":"commentary","content":[{"type":"output_text","text":"Searching"}]}) }),
        owned(Part::ServerToolCall { server_tool_call: json!({"type":"reasoning","encrypted_content":"private-ciphertext"}) }),
    ] });
    let record = from_adk(&db, TurnId::new(), &event).await.unwrap();
    let message = record.message().unwrap();
    assert_eq!(message.part_count, 6);
    assert_eq!(
        message
            .content
            .iter()
            .map(|block| block.index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4]
    );
    let BlockKind::RedactedThinking { data, before } = &message.content[2].kind else {
        panic!("redacted thinking expected")
    };
    assert_eq!(data, &["first", "second"]);
    assert_eq!(before, &[Some(0), Some(1)]);
    let encoded = serde_json::to_string(&record).unwrap();
    assert!(encoded.contains("public query"));
    assert!(encoded.contains("Searching"));
    assert!(!encoded.contains("provider_data"));
    assert!(!encoded.contains("anthropic_message"));
    assert!(!encoded.contains("private-ciphertext"));
    let restored = to_adk(&db, &record).await.unwrap();
    let Part::ServerToolResponse {
        server_tool_response,
    } = &restored.content().unwrap().parts[2]
    else {
        panic!("native boundary expected")
    };
    let content = server_tool_response["content"].as_array().unwrap();
    assert_eq!(content[0]["data"], "first");
    assert_eq!(content[1]["type"], "tool_use");
    assert_eq!(content[2]["data"], "second");
    assert_eq!(content[3]["text"], "Answer");
}

#[tokio::test]
async fn captures_incoming_tool_metadata() {
    let (_database, db) = database().await;
    let mut event = Event::new("gdk");
    event.set_content(Content {
        role: "model".into(),
        parts: vec![Part::FunctionCall {
            id: Some("call".into()),
            name: "read".into(),
            args: json!({"path":"file"}),
            thought_signature: Some("signature".into()),
        }],
    });
    event.llm_response.provider_metadata = Some(json!({"gdk_tool_metadata":{"call":{
        "provider":{"thoughtSignature":"signature","other":"value"},"tool_meta":{"title":"File"}
    }}}));
    let record = from_adk(&db, TurnId::new(), &event).await.unwrap();
    let reloaded: Record = serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
    let BlockKind::ToolRequest(call) = &reloaded.message().unwrap().content[0].kind else {
        panic!("tool request expected");
    };
    assert_eq!(call.signature.as_deref(), Some("signature"));
    assert_eq!(call.provider.as_ref().unwrap()["other"], "value");
    assert_eq!(call.tool_meta.as_ref().unwrap()["title"], "File");
}
