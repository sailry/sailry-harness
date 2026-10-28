use super::*;

async fn database() -> (turso::Database, Connection) {
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

#[tokio::test]
async fn preserves_call_metadata() {
    let (_database, db) = database().await;
    let mut message = gdk::Message::assistant()
        .with_generated_id()
        .with_tool_request(
            "call-1",
            Ok(CallToolRequestParams::new("read_file")
                .with_arguments(serde_json::from_value(json!({"path":"one"})).unwrap())),
        );
    let MessageContent::ToolRequest(call) = &mut message.content[0] else {
        unreachable!()
    };
    call.metadata = Some(
        serde_json::from_value(json!({"thoughtSignature":"signed","nested":{"opaque":[1,true]}}))
            .unwrap(),
    );
    call.tool_meta = Some(json!({"title":"Read","foreign":{"count":2}}));
    message
        .metadata
        .set_operation_note("foreground_subagent", "delivered", json!("child"));
    let record = capture(&db, TurnId::new(), &message, &BTreeMap::new())
        .await
        .unwrap();
    let stored: Record = serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
    let projected = project(&db, &stored).await.unwrap().unwrap();
    assert_eq!(projected, message);
    let BlockKind::ToolRequest(call) = &stored.message().unwrap().content[0].kind else {
        unreachable!()
    };
    assert_eq!(call.id.as_deref(), Some("call-1"));
}

#[tokio::test]
async fn preserves_structured_output() {
    let (_database, db) = database().await;
    let value = json!({"result":"exact","inline":{"mime_type":"image/png","data":"AQID"}});
    let mut result = CallToolResult::structured(value.clone());
    result.meta = Some(
        serde_json::from_value(json!({"foreground_subagent":true,"subagent_session_id":"child"}))
            .unwrap(),
    );
    let message = gdk::Message::user()
        .with_generated_id()
        .with_tool_response("call", Ok(result));
    let record = capture(
        &db,
        TurnId::new(),
        &message,
        &BTreeMap::from([("call".into(), "read_file".into())]),
    )
    .await
    .unwrap();
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
    assert_eq!(project(&db, &record).await.unwrap().unwrap(), message);
}

#[tokio::test]
async fn preserves_mcp_blocks() {
    let (_database, db) = database().await;
    let result:CallToolResult=serde_json::from_value(json!({"content":[{"type":"text","text":"different"},{"type":"image","mimeType":"image/png","data":"AQID"}],"structuredContent":{"value":1},"_meta":{"opaque":"same"}})).unwrap();
    let message = gdk::Message::user()
        .with_generated_id()
        .with_tool_response("call", Ok(result));
    let record = capture(&db, TurnId::new(), &message, &BTreeMap::new())
        .await
        .unwrap();
    let BlockKind::ToolResponse(response) = &record.message().unwrap().content[0].kind else {
        unreachable!()
    };
    assert_eq!(response.format, ToolFormat::Mcp);
    assert_eq!(project(&db, &record).await.unwrap().unwrap(), message);
}

#[tokio::test]
async fn preserves_explicit_mcp_success() {
    let (_database, db) = database().await;
    let mut result = CallToolResult::structured(serde_json::Value::Null);
    result.is_error = Some(false);
    result.meta = Some(serde_json::from_value(json!({"opaque":null})).unwrap());
    let message = gdk::Message::user()
        .with_generated_id()
        .with_tool_response("call", Ok(result));
    let record = capture(&db, TurnId::new(), &message, &BTreeMap::new())
        .await
        .unwrap();
    let BlockKind::ToolResponse(response) = &record.message().unwrap().content[0].kind else {
        unreachable!()
    };
    assert_eq!(response.format, ToolFormat::Mcp);
    assert_eq!(project(&db, &record).await.unwrap().unwrap(), message);
}

#[tokio::test]
async fn rejects_missing_call_identity() {
    let (_database, db) = database().await;
    let message = gdk::Message::assistant()
        .with_generated_id()
        .with_tool_request("", Ok(CallToolRequestParams::new("read_file")));
    assert!(
        capture(&db, TurnId::new(), &message, &BTreeMap::new())
            .await
            .is_err()
    );
}
