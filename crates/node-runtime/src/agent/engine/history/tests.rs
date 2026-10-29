use super::*;

#[test]
fn preserves_optional_reasoning_usage() {
    use goose_provider_types::conversation::token_usage::{ProviderUsage, Usage};
    let mut source = ProviderUsage::new("fixture".into(), Usage::new(Some(12), Some(3), Some(15)));
    assert_eq!(provider_usage(&source).unwrap().reasoning, None);
    for (value, expected) in [
        (json!(0), Some(0)),
        (json!(2), Some(2)),
        (json!(u64::MAX), Some(u64::MAX)),
        (Value::Null, None),
        (json!(-1), None),
        (json!(1.5), None),
        (json!("2"), None),
    ] {
        source.additional_data = Some([("reasoning_tokens".into(), value)].into_iter().collect());
        let usage = provider_usage(&source).unwrap();
        assert_eq!(usage.reasoning, expected);
        assert_eq!((usage.input, usage.output), (Some(12), Some(3)));
    }
}

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
    assert_eq!(stored.provenance.author, "assistant");
    let projected = project(&db, &stored).await.unwrap().unwrap();
    assert_eq!(projected, message);
    let BlockKind::ToolRequest(call) = &stored.message().unwrap().content[0].kind else {
        unreachable!()
    };
    assert_eq!(call.id.as_str(), "call-1");
    assert_eq!(
        call.provider,
        message.content[0].as_tool_request().unwrap().metadata
    );
    let value = serde_json::to_value(call).unwrap();
    assert!(value.get("signature").is_none());
    assert_eq!(value["provider"]["thoughtSignature"], "signed");
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
    let mut message = gdk::Message::user()
        .with_generated_id()
        .with_tool_response("call", Ok(result));
    let MessageContent::ToolResponse(response) = &mut message.content[0] else {
        panic!("tool response expected")
    };
    response.metadata =
        Some(serde_json::from_value(json!({"opaque":{"retained":[1,true]}})).unwrap());
    let record = capture(
        &db,
        TurnId::new(),
        &message,
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
    assert_eq!(response.attachments.len(), 1);
    let Attachment::Inline { asset } = &response.attachments[0];
    assert_eq!(asset.digest, response.result.assets[0].asset.digest);
    assert_eq!(blobs::get(&db, &asset.digest).await.unwrap(), [1, 2, 3]);
    assert_eq!(project(&db, &record).await.unwrap().unwrap(), message);
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
        let mut result = CallToolResult::structured(serde_json::Value::Null);
        result.is_error = is_error;
        if !result_type {
            result.result_type = None;
        }
        result.meta = Some(serde_json::from_value(json!({"opaque":null})).unwrap());
        let message = gdk::Message::user()
            .with_generated_id()
            .with_tool_response("call", Ok(result));
        let record = capture(&db, TurnId::new(), &message, &BTreeMap::new())
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
        assert_eq!(project(&db, &stored).await.unwrap().unwrap(), message);
    }
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

#[tokio::test]
async fn projects_declared_tool_media() {
    let (_database, db) = database().await;
    let value = json!({"response":{"text":"Exact metadata"},"inline_data":[{"mime_type":"image/png","data":[1,2,3]}]});
    let message = gdk::Message::user()
        .with_generated_id()
        .with_tool_response("capture", Ok(CallToolResult::structured(value.clone())));
    let record = capture(
        &db,
        TurnId::new(),
        &message,
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
    let MessageContent::ToolResponse(response) = &projected.content[0] else {
        unreachable!()
    };
    let result = response.tool_result.as_ref().unwrap();
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
    let mut message = gdk::Message::user().with_generated_id();
    message.content.push(MessageContent::Image(
        serde_json::from_value(json!({
            "type":"image", "data":"AQID", "mimeType":"image/png",
            "annotations":{"audience":["user"], "priority":0.8}
        }))
        .unwrap(),
    ));
    message
        .content
        .push(MessageContent::Document(gdk::DocumentContent {
            data: "BAU=".into(),
            mime_type: "application/pdf".into(),
            name: Some("report.pdf".into()),
        }));
    let record = capture(&db, TurnId::new(), &message, &BTreeMap::new())
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
    assert_eq!(project(&db, &stored).await.unwrap().unwrap(), message);
}
