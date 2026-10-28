use super::*;

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

fn body(parts: Option<Value>) -> Value {
    json!({
        "id":"original-event", "timestamp":"2026-01-02T03:04:05Z",
        "invocation_id":"original-turn", "author":"assistant", "branch":"worker.child",
        "content":parts.map(|parts| json!({"role":"model","parts":parts})),
        "partial":false, "turn_complete":false, "interrupted":false,
        "actions":{"state_delta":{},"artifact_delta":{},"skip_summarization":false,
            "transfer_to_agent":null,"escalate":false},
        "event_metadata":{}
    })
}

async fn read(db: &Connection, body: &Value) -> Result<Record, Fault> {
    capture(db, TurnId::new(), &serde_json::to_vec(body).unwrap()).await
}

#[tokio::test]
async fn retains_binary_output_and_references() {
    let (_database, db) = database().await;
    let raw = json!({
        "content":[{"type":"image","mimeType":"image/png","data":"AAECAw=="}],
        "unknown":{"exact":[null,false,3,"four"]}
    });
    let mut event = body(Some(json!([
        {"name":"read_file","args":{"path":"a.txt"},"id":"call","thought_signature":"signed"},
        {"functionResponse":{"name":"read_file","response":raw,
            "inline_data":[{"mime_type":"image/png","data":[4,5],"uri":"capture.png"}],
            "file_data":[{"mime_type":"text/plain","file_uri":"file:///a.txt"}]},
            "id":"call","annotations":{"adk.tool_result.status":"failed"}},
        {"mime_type":"image/png","data":[6,7,8],"uri":"input.png"}
    ])));
    event["content"]["role"] = json!("function");
    let record = read(&db, &event).await.unwrap();
    assert_eq!(record.id, "original-event");
    assert_eq!(record.created.to_rfc3339(), "2026-01-02T03:04:05+00:00");
    assert_eq!(record.provenance.author, "assistant");
    assert_eq!(record.provenance.branch, "worker.child");
    let message = record.message().unwrap();
    assert_eq!(message.role, Role::User);
    assert_eq!(message.part_count, 3);
    let BlockKind::ToolRequest(call) = &message.content[0].kind else {
        panic!("tool request expected")
    };
    assert_eq!(call.id.as_deref(), Some("call"));
    assert_eq!(call.signature.as_deref(), Some("signed"));
    let BlockKind::ToolResponse(result) = &message.content[1].kind else {
        panic!("tool response expected")
    };
    assert_eq!(result.status, ToolStatus::Error);
    assert_eq!(
        result.revision,
        blake3::hash(&serde_json::to_vec(&raw).unwrap())
            .to_hex()
            .to_string()
    );
    assert_eq!(blobs::hydrate(&db, &result.result).await.unwrap(), raw);
    assert_eq!(result.result.assets.len(), 1);
    let [Attachment::Inline { asset }, Attachment::File { file }] = result.attachments.as_slice()
    else {
        panic!("ordered attachments expected")
    };
    assert_eq!(blobs::get(&db, &asset.digest).await.unwrap(), [4, 5]);
    assert_eq!(asset.uri.as_deref(), Some("capture.png"));
    assert_eq!(file.uri, "file:///a.txt");
    let BlockKind::Image { asset } = &message.content[2].kind else {
        panic!("inline image expected")
    };
    assert_eq!(blobs::get(&db, &asset.digest).await.unwrap(), [6, 7, 8]);
    assert_eq!(asset.uri.as_deref(), Some("input.png"));
    crate::agent::canonical::validate(&record).unwrap();
}

#[tokio::test]
async fn retains_slots_and_public_native_content() {
    let (_database, db) = database().await;
    let owned = |part| json!({"provider_data":{"origin":null,"parts":[part]}});
    let mut event = body(Some(json!([
        {"configuration_update":{"effort":"low"}},
        owned(json!({"name":"read_file","args":{},"id":"call"})),
        {"text":"Answer"},
        owned(json!({"server_tool_response":{"type":"anthropic_message","content":[
            {"type":"redacted_thinking","data":"first"},
            {"type":"tool_use","id":"call","name":"read_file","input":{}},
            {"type":"redacted_thinking","data":"second"},
            {"type":"text","text":"Answer"}
        ]}})),
        owned(json!({"server_tool_call":{"type":"web_search_call","id":"search","query":"public query"}})),
        owned(json!({"server_tool_call":{"type":"message","phase":"commentary","content":[{"text":"Searching"}]}})),
        owned(json!({"server_tool_call":{"type":"reasoning","encrypted_content":"discarded-ciphertext"}})),
        {"configuration_update":{"effort":"high"}}
    ])));
    event["event_metadata"]["sailry.context.notice_ids"] = json!("[]");
    let record = read(&db, &event).await.unwrap();
    let message = record.message().unwrap();
    assert_eq!(message.part_count, 8);
    assert_eq!(
        message
            .content
            .iter()
            .map(|block| block.index)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 5]
    );
    assert_eq!(record.metadata.input.as_ref().unwrap().notice_part, Some(1));
    let BlockKind::RedactedThinking { data, before } = &message.content[2].kind else {
        panic!("redacted thinking expected")
    };
    assert_eq!(data, &["first", "second"]);
    assert_eq!(before, &[Some(1), Some(2)]);
    let encoded = serde_json::to_string(&record).unwrap();
    for retained in ["public query", "Searching", "read_file"] {
        assert!(encoded.contains(retained));
    }
    for removed in [
        "configuration_update",
        "provider_data",
        "anthropic_message",
        "discarded-ciphertext",
    ] {
        assert!(!encoded.contains(removed));
    }
    crate::agent::canonical::validate(&record).unwrap();
}

#[tokio::test]
async fn retains_context_state_and_visibility() {
    let (_database, db) = database().await;
    let mut event = body(None);
    event["author"] = json!("system");
    event["event_metadata"]["sailry_context_projection"] = json!("{\"tokens\":42}");
    assert!(
        !read(&db, &event)
            .await
            .unwrap()
            .metadata
            .runtime
            .unwrap()
            .user_visible
    );
    event["actions"]["unknown_action"] = json!(true);
    assert!(
        read(&db, &event)
            .await
            .unwrap()
            .metadata
            .runtime
            .unwrap()
            .user_visible
    );
    event["actions"]
        .as_object_mut()
        .unwrap()
        .remove("unknown_action");
    event["event_metadata"]["sailry.context"] = json!("context");
    event["actions"]["state_delta"] = json!({
        "sailry.context.policy":"Policy", "sailry.context.environment":"Environment",
        "sailry.context.guide:original":{"path":"AGENTS.md","revision":"revision","content":"Instructions"},
        "sailry.context.loaded_tools":["read_file"],
        "sailry.context.masked_outputs":[["old-event","old-call"]],
        "saved":{"value":1},"app:shared":2,"user:preference":null,"temp:transient":3
    });
    let record = read(&db, &event).await.unwrap();
    assert!(matches!(record.kind, Kind::Configuration(_)));
    let context = record.context().unwrap();
    assert!(context.captured);
    assert_eq!(context.policy.as_deref(), Some("Policy"));
    assert_eq!(context.environment.as_deref(), Some("Environment"));
    assert_eq!(
        context.guides["sailry.context.guide:original"].revision,
        "revision"
    );
    assert!(context.loaded_tools.as_ref().unwrap().contains("read_file"));
    let state = record.metadata.state.as_ref().unwrap();
    assert_eq!(state.session, [("saved".into(), json!({"value":1}))].into());
    assert_eq!(state.application, [("shared".into(), json!(2))].into());
    assert_eq!(state.user, [("preference".into(), Value::Null)].into());
    assert!(
        !serde_json::to_string(&record)
            .unwrap()
            .contains("masked_outputs")
    );
    assert!(record.metadata.runtime.as_ref().unwrap().user_visible);
}

#[tokio::test]
async fn reads_original_elicitation_identity() {
    let (_database, db) = database().await;
    let mut event = body(None);
    event["event_metadata"]["sailry.mcp.elicitation"] = json!(serde_json::to_string(&json!({
        "parent":{"tool_name":"external_tool","args":{"choice":"original"},"function_call_id":"call"},
        "request":{"message":"Choose"},
        "spec":{"prompt":"Choose","input":{"kind":"text","multiline":false,"max_bytes":64}},
        "schema":{"type":"object"}
    })).unwrap());
    let record = read(&db, &event).await.unwrap();
    let Kind::Notice(Notice::Elicitation {
        parent,
        request,
        schema,
        ..
    }) = &record.kind
    else {
        panic!("elicitation notice expected")
    };
    assert_eq!(parent.id.as_deref(), Some("call"));
    assert_eq!(parent.name, "external_tool");
    assert_eq!(parent.arguments, json!({"choice":"original"}));
    assert_eq!(request, &json!({"message":"Choose"}));
    assert_eq!(schema, &json!({"type":"object"}));
    event["content"] = json!({"role":"model","parts":[{"text":"Unexpected model content"}]});
    assert!(read(&db, &event).await.is_err());
}

#[tokio::test]
async fn retains_usage_sources_and_summary() {
    let (_database, db) = database().await;
    let mut event = body(None);
    event["usage_metadata"] = json!({"prompt_token_count":-1,"candidates_token_count":12,"total_token_count":12,
        "cache_read_input_token_count":7,"thinking_token_count":3,"provider_usage":{"reported":true},
        "builtin_tool_uses":[{"kind":"web_search","count":2}]});
    event["citation_metadata"] = json!({"citationSources":[{"uri":"https://example.com","title":"Source","startIndex":2,"endIndex":4}]});
    event["provider_metadata"] = json!({"groundingChunks":[{"web":{"uri":"https://example.org","title":"Grounding"}}],
        "searchEntryPoint":{"renderedContent":"Search suggestions"},"sailry_timing":{"elapsed_us":90}});
    event["actions"]["compaction"] = json!({
        "start_timestamp":"2026-01-01T00:00:00Z","end_timestamp":"2026-01-02T00:00:00Z",
        "compacted_content":{"role":"model","parts":[
            {"provider_data":{"origin":{"provider":"openai","model":"model","scope":"scope"},"parts":[{"configuration_update":{"effort":"low"}}]}},
            {"text":"Handoff"}
        ]}
    });
    event["event_metadata"]["sailry.context.end_event"] = json!("boundary-event");
    event["event_metadata"]["sailry.compaction.files"] = json!("{\"src/lib.rs\":true}");
    let record = read(&db, &event).await.unwrap();
    let Kind::Compaction(compaction) = &record.kind else {
        panic!("compaction record expected")
    };
    assert_eq!(compaction.boundary, "boundary-event");
    assert_eq!(compaction.summary.role, Role::User);
    assert_eq!(compaction.summary.part_count, 1);
    assert!(
        matches!(&compaction.summary.content[0].kind, BlockKind::Text { text } if text == "Handoff")
    );
    assert_eq!(compaction.files[0].path, "src/lib.rs");
    assert_eq!(compaction.files[0].modified, Some(true));
    let usage = compaction.summary.usage.as_ref().unwrap();
    assert_eq!(usage.input, None);
    assert_eq!(usage.output, Some(12));
    assert_eq!(usage.cache_read, Some(7));
    assert_eq!(usage.reasoning, Some(3));
    assert_eq!(usage.provider, Some(json!({"reported":true})));
    assert_eq!(usage.builtins[0].count, 2);
    assert_eq!(compaction.summary.sources.citations[0].start, Some(2));
    assert_eq!(
        compaction.summary.sources.grounding[0].uri,
        "https://example.org"
    );
    assert_eq!(
        compaction.summary.sources.search_suggestions.as_deref(),
        Some("Search suggestions")
    );
    assert_eq!(
        record
            .metadata
            .execution
            .as_ref()
            .unwrap()
            .timing
            .as_ref()
            .unwrap()
            .elapsed_us,
        Some(90)
    );
    crate::agent::canonical::validate(&record).unwrap();
}

#[tokio::test]
async fn rejects_unknown_parts_and_empty_envelopes() {
    let (_database, db) = database().await;
    for parts in [
        json!([{"unknown_part":true}]),
        json!([{"provider_data":{"origin":null,"parts":[]}}]),
    ] {
        assert!(read(&db, &body(Some(parts))).await.is_err());
    }
}
