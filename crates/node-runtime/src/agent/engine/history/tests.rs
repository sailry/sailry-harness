use super::*;

#[test]
fn preserves_optional_reasoning_usage() {
    for reasoning in [None, Some(0), Some(2), Some(u64::MAX)] {
        let source = model::Usage {
            model: Some("fixture".into()),
            input: Some(12),
            output: Some(3),
            total: Some(15),
            reasoning,
            ..model::Usage::default()
        };
        let usage = provider_usage(&source).unwrap();
        assert_eq!(usage.reasoning, reasoning);
        assert_eq!((usage.input, usage.output), (Some(12), Some(3)));
    }
}

pub(super) async fn database() -> (turso::Database, Connection) {
    let database = turso::Builder::new_local(":memory:").build().await.unwrap();
    let db = database.connect().unwrap();
    db.execute(
        "CREATE TABLE agent_blobs(digest TEXT PRIMARY KEY,bytes BLOB NOT NULL)",
        (),
    )
    .await
    .unwrap();
    (database, db)
}

fn message(role: model::Role, content: Vec<Content>) -> model::Message {
    model::Message {
        id: Some("message".into()),
        role,
        content,
        ..model::Message::default()
    }
}

fn result(id: &str, name: &str, result: CallToolResult) -> Content {
    Content::ToolResult(model::ToolResult {
        id: id.into(),
        name: name.into(),
        result: Ok(result),
        metadata: None,
    })
}

fn without_output_notes(mut message: model::Message) -> model::Message {
    if let Some(notes) = message.metadata.operations.as_mut() {
        notes.remove("tool_output");
        if notes.is_empty() {
            message.metadata.operations = None;
        }
    }
    message
}

#[tokio::test]
async fn preserves_provider_failure() {
    let (_database, db) = database().await;
    for reason in [
        "Could not connect to example.test — check your network connection and try again.",
        "Request timed out — check your network connection and try again.",
        "Provider diagnostic\n\nPlease resend your message to try again.",
    ] {
        let source = model::Message::assistant()
            .with_generated_id()
            .with_error(model::ErrorKind::Network, reason);
        let record = capture(&db, TurnId::new(), &source, &BTreeMap::new())
            .await
            .unwrap();
        let stored = record.message().unwrap();
        assert_eq!(stored.finish, Some(Finish::Failed));
        let failure = stored.error.as_ref().unwrap();
        assert_eq!(failure.code.as_deref(), Some("other"));
        assert_eq!(failure.message, reason);
        assert_eq!(project(&db, &record).await.unwrap().unwrap(), source);
    }
}

#[tokio::test]
async fn preserves_other_failure_content() {
    let (_database, db) = database().await;
    let text = "Network error: quoted diagnostic\n\nPlease resend your message to try again.";
    for source in [
        message(model::Role::User, vec![Content::Text { text: text.into() }]),
        message(
            model::Role::Assistant,
            vec![Content::Text { text: text.into() }],
        ),
        message(
            model::Role::Assistant,
            vec![Content::Error {
                code: Some("authentication".into()),
                message: text.into(),
            }],
        ),
        message(
            model::Role::Assistant,
            vec![Content::Error {
                code: Some("other".into()),
                message: "Other diagnostic\n\nPlease resend your message to try again.".into(),
            }],
        ),
        message(
            model::Role::Assistant,
            vec![Content::Error {
                code: Some("other".into()),
                message: format!("{text}\nAdditional provider details"),
            }],
        ),
    ] {
        let record = capture(&db, TurnId::new(), &source, &BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(project(&db, &record).await.unwrap().unwrap(), source);
    }
}

#[tokio::test]
async fn preserves_prompt_context() {
    let (_database, db) = database().await;
    for retained in [vec![], vec!["part-one".to_owned(), "part-two".to_owned()]] {
        let context = PromptContext {
            state: "session-state".into(),
            retained,
        };
        let mut source = message(
            model::Role::User,
            vec![Content::Text {
                text: "<turn-context>\nEnvironment\n\nPart\n</turn-context>".into(),
            }],
        );
        source.metadata.user_visible = false;
        source.metadata.turn_context = true;
        source
            .metadata
            .set_operation_note("prompt", "context", json!(context));
        let record = capture(&db, TurnId::new(), &source, &BTreeMap::new())
            .await
            .unwrap();
        let stored: Record = serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
        assert_eq!(
            stored
                .metadata
                .runtime
                .as_ref()
                .unwrap()
                .notes
                .as_ref()
                .unwrap()
                .context
                .as_ref(),
            Some(&context)
        );
        assert_eq!(project(&db, &stored).await.unwrap().unwrap(), source);
    }
}

#[tokio::test]
async fn preserves_runtime_metadata() {
    let (_database, db) = database().await;
    let mut source = message(
        model::Role::Assistant,
        vec![
            Content::RedactedThinking {
                data: "sealed".into(),
            },
            Content::Thinking {
                text: "Thinking".into(),
                signature: Some("signed".into()),
            },
            Content::Text {
                text: "Answer".into(),
            },
        ],
    );
    source.created = 1_700_000_000;
    source.metadata = model::Metadata {
        user_visible: false,
        output_token_limit_reached: true,
        steer: true,
        turn_context: true,
        inference: Some(model::Inference {
            provider: "provider".into(),
            requested_model: "requested".into(),
            resolved_model: Some("resolved".into()),
            provider_session_id: Some("provider-session".into()),
        }),
        usage: Some(Box::new(model::Usage {
            model: Some("resolved".into()),
            input: Some(u64::from(u32::MAX)),
            output: Some(7),
            total: Some(u64::from(u32::MAX) + 7),
            cache_read: Some(9),
            cache_write: Some(3),
            reasoning: Some(2),
            cost: Some(0.25),
            provider: Some(json!({"opaque":true})),
            elapsed_ms: Some(150),
            time_to_first_token_ms: Some(12),
            ..model::Usage::default()
        })),
        operation_logs: vec!["operation".into()],
        ..model::Metadata::default()
    };
    for (operation, key, value) in [
        ("tool_pair_compaction", "after", json!("previous")),
        ("inference", "empty_response", json!(true)),
        ("foreground_subagent", "delivered", json!("child")),
        ("foreground_subagent", "cancelled", json!(["cancelled"])),
    ] {
        source.metadata.set_operation_note(operation, key, value);
    }
    let record = capture(&db, TurnId::new(), &source, &BTreeMap::new())
        .await
        .unwrap();
    let stored: Record = serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
    assert_eq!(project(&db, &stored).await.unwrap().unwrap(), source);
}

#[tokio::test]
async fn preserves_call_metadata() {
    let (_database, db) = database().await;
    let provider = Some(
        serde_json::from_value(json!({"thoughtSignature":"signed","nested":{"opaque":[1,true]}}))
            .unwrap(),
    );
    let mut source = message(
        model::Role::Assistant,
        vec![Content::ToolCall(model::ToolCall {
            id: "call-1".into(),
            name: "read_file".into(),
            arguments: json!({"path":"one"}),
            metadata: provider.clone(),
            tool_meta: Some(json!({"title":"Read","foreign":{"count":2},EXECUTABLE_KEY:true})),
            error: None,
        })],
    );
    source
        .metadata
        .set_operation_note("foreground_subagent", "delivered", json!("child"));
    let record = capture(&db, TurnId::new(), &source, &BTreeMap::new())
        .await
        .unwrap();
    let stored: Record = serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
    assert_eq!(stored.provenance.author, "assistant");
    assert_eq!(project(&db, &stored).await.unwrap().unwrap(), source);
    let BlockKind::ToolRequest(call) = &stored.message().unwrap().content[0].kind else {
        unreachable!()
    };
    assert_eq!(call.id.as_str(), "call-1");
    assert_eq!(call.provider, provider);
    let value = serde_json::to_value(call).unwrap();
    assert!(value.get("signature").is_none());
    assert_eq!(value["provider"]["thoughtSignature"], "signed");
}

#[tokio::test]
async fn preserves_unnamed_invalid_calls() {
    let (_database, db) = database().await;
    let source = message(
        model::Role::Assistant,
        vec![Content::ToolCall(model::ToolCall {
            id: "call".into(),
            name: String::new(),
            arguments: Value::Null,
            metadata: Some(serde_json::from_value(json!({"opaque":true})).unwrap()),
            tool_meta: None,
            error: Some(model::ToolError {
                code: -32602,
                message: "incomplete arguments".into(),
                data: Some(json!({"received":"{"})),
            }),
        })],
    );
    let record = capture(&db, TurnId::new(), &source, &BTreeMap::new())
        .await
        .unwrap();
    let stored: Record = serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
    assert_eq!(project(&db, &stored).await.unwrap().unwrap(), source);
}

#[tokio::test]
async fn preserves_structured_output() {
    let (_database, db) = database().await;
    let value = json!({"result":"exact","inline":{"mime_type":"image/png","data":"AQID"}});
    let mut output = CallToolResult::structured(value.clone());
    output.meta = Some(
        serde_json::from_value(json!({"foreground_subagent":true,"subagent_session_id":"child"}))
            .unwrap(),
    );
    let mut source = message(model::Role::User, vec![result("call", "read_file", output)]);
    let Content::ToolResult(response) = &mut source.content[0] else {
        panic!("tool response expected")
    };
    response.metadata =
        Some(serde_json::from_value(json!({"opaque":{"retained":[1,true]}})).unwrap());
    let record = capture(
        &db,
        TurnId::new(),
        &source,
        &BTreeMap::from([("call".into(), "read_file".into())]),
    )
    .await
    .unwrap();
    assert_eq!(record.provenance.author, "assistant");
    assert_eq!(record.message().unwrap().role, Role::User);
    let BlockKind::ToolResponse(response) = &record.message().unwrap().content[0].kind else {
        unreachable!()
    };
    assert_eq!(response.format, ToolFormat::Json);
    assert_eq!(response.result.assets.len(), 1);
    assert_eq!(
        response.revision,
        blake3::hash(value.to_string().as_bytes())
            .to_hex()
            .to_string()
    );
    assert_eq!(blobs::hydrate(&db, &response.result).await.unwrap(), value);
    assert_eq!(
        without_output_notes(project(&db, &record).await.unwrap().unwrap()),
        source
    );
}

#[tokio::test]
async fn preserves_mcp_blocks() {
    let (_database, db) = database().await;
    let output: CallToolResult = serde_json::from_value(json!({"content":[{"type":"text","text":"different"},{"type":"image","mimeType":"image/png","data":"AQID"}],"structuredContent":{"value":1},"_meta":{"opaque":"same"}})).unwrap();
    let source = message(model::Role::User, vec![result("call", "", output)]);
    let record = capture(&db, TurnId::new(), &source, &BTreeMap::new())
        .await
        .unwrap();
    let BlockKind::ToolResponse(response) = &record.message().unwrap().content[0].kind else {
        unreachable!()
    };
    assert_eq!(response.format, ToolFormat::Mcp);
    assert_eq!(response.attachments.len(), 1);
    let Attachment::Inline { asset } = &response.attachments[0];
    assert_eq!(asset.digest, response.result.assets[0].asset.digest);
    assert_eq!(blobs::get(&db, &asset.digest).await.unwrap(), [1, 2, 3]);
    assert_eq!(
        without_output_notes(project(&db, &record).await.unwrap().unwrap()),
        source
    );
}

#[tokio::test]
async fn preserves_result_flags() {
    let (_database, db) = database().await;
    for (is_error, result_type, format) in [
        (Some(false), true, ToolFormat::Json),
        (None, true, ToolFormat::Mcp),
        (Some(false), false, ToolFormat::Mcp),
        (Some(true), true, ToolFormat::Mcp),
    ] {
        let mut output = CallToolResult::structured(Value::Null);
        output.is_error = is_error;
        if !result_type {
            output.result_type = None;
        }
        output.meta = Some(serde_json::from_value(json!({"opaque":null})).unwrap());
        let source = message(model::Role::User, vec![result("call", "", output)]);
        let record = capture(&db, TurnId::new(), &source, &BTreeMap::new())
            .await
            .unwrap();
        let stored: Record = serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
        let BlockKind::ToolResponse(response) = &stored.message().unwrap().content[0].kind else {
            unreachable!()
        };
        assert_eq!(response.format, format);
        if format == ToolFormat::Json {
            assert_eq!(
                blobs::hydrate(&db, &response.result).await.unwrap(),
                Value::Null
            );
            assert_eq!(
                response.meta,
                Some(serde_json::from_value(json!({"opaque":null})).unwrap())
            );
        }
        assert_eq!(
            without_output_notes(project(&db, &stored).await.unwrap().unwrap()),
            source
        );
    }
}

#[tokio::test]
async fn rejects_missing_call_identity() {
    let (_database, db) = database().await;
    let source = message(
        model::Role::Assistant,
        vec![Content::ToolCall(model::ToolCall {
            id: String::new(),
            name: "read_file".into(),
            arguments: json!({}),
            metadata: None,
            tool_meta: None,
            error: None,
        })],
    );
    assert!(
        capture(&db, TurnId::new(), &source, &BTreeMap::new())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn projects_declared_tool_media() {
    let (_database, db) = database().await;
    let value = json!({"response":{"text":"Exact metadata"},"inline_data":[{"mime_type":"image/png","data":[1,2,3]}]});
    let source = message(
        model::Role::User,
        vec![result(
            "capture",
            "computer",
            CallToolResult::structured(value.clone()),
        )],
    );
    let record = capture(
        &db,
        TurnId::new(),
        &source,
        &BTreeMap::from([("capture".into(), "computer".into())]),
    )
    .await
    .unwrap();
    let BlockKind::ToolResponse(response) = &record.message().unwrap().content[0].kind else {
        unreachable!()
    };
    assert_eq!(blobs::hydrate(&db, &response.result).await.unwrap(), value);
    assert_eq!(
        response.revision,
        blake3::hash(value.to_string().as_bytes())
            .to_hex()
            .to_string()
    );
    let projected = project(&db, &record).await.unwrap().unwrap();
    let Content::ToolResult(response) = &projected.content[0] else {
        unreachable!()
    };
    let result = response.result.as_ref().unwrap();
    assert!(
        result.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("Exact metadata")
    );
    assert!(
        !result.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("inline_data")
    );
    let rmcp::model::ContentBlock::Image(image) = &result.content[1] else {
        panic!("image expected")
    };
    assert_eq!(image.data, "AQID");
    assert_eq!(image.mime_type, "image/png");
}

#[tokio::test]
async fn preserves_media_metadata() {
    let (_database, db) = database().await;
    let source = message(
        model::Role::User,
        vec![
            Content::Image {
                media_type: "image/png".into(),
                data: model::Image::Inline {
                    data_base64: "AQID".into(),
                },
                annotations: Some(json!({"audience":["user"], "priority":0.8})),
            },
            Content::Document {
                data_base64: "BAU=".into(),
                media_type: "application/pdf".into(),
                name: Some("report.pdf".into()),
            },
        ],
    );
    let record = capture(&db, TurnId::new(), &source, &BTreeMap::new())
        .await
        .unwrap();
    let stored: Record = serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
    let BlockKind::Image { asset } = &stored.message().unwrap().content[0].kind else {
        panic!("image expected")
    };
    assert_eq!(
        asset.annotations.as_ref().unwrap()["audience"],
        json!(["user"])
    );
    let BlockKind::Document { asset } = &stored.message().unwrap().content[1].kind else {
        panic!("document expected")
    };
    assert_eq!(asset.uri.as_deref(), Some("report.pdf"));
    assert_eq!(project(&db, &stored).await.unwrap().unwrap(), source);
}

#[tokio::test]
async fn caches_immutable_projection() {
    let (_database, db) = database().await;
    let source = model::Message::user()
        .with_generated_id()
        .with_image("AQID", "image/png")
        .with_tool_response(
            "result",
            Ok(CallToolResult::success(vec![
                rmcp::model::ContentBlock::image("BAU=", "image/png"),
            ])),
        );
    let record = capture(&db, TurnId::new(), &source, &BTreeMap::new())
        .await
        .unwrap();
    let mut cache = Cache::default();
    let expected = cache.project(&db, 1, &record).await.unwrap().unwrap();
    db.execute("DELETE FROM agent_blobs", ()).await.unwrap();
    assert!(project(&db, &record).await.is_err());
    let mut first = cache.project(&db, 1, &record).await.unwrap().unwrap();
    assert_eq!(first, expected);
    first.metadata.agent_visible = false;
    first.content.clear();
    assert_eq!(
        cache.project(&db, 1, &record).await.unwrap().unwrap(),
        expected
    );
    assert!(cache.project(&db, 2, &record).await.is_err());
    let mut other = record.clone();
    other.id = "another-record".into();
    assert!(cache.project(&db, 1, &other).await.is_err());
}
