use super::*;

#[test]
fn pages_original_unicode_json() {
    let session = SessionId::new();
    let turn = TurnId::new();
    let value = serde_json::json!({"text":"中文🙂\n".repeat(1000),"value":42});
    let original = value.to_string();
    let output = OutputReference {
        call: "call".into(),
        revision: blake3::hash(original.as_bytes()).to_hex().to_string(),
    };
    let mut combined = String::new();
    let mut offset = 0;
    loop {
        let page = page(
            Resolved {
                session,
                turn,
                value: value.clone(),
            },
            output.clone(),
            offset,
            7,
        )
        .unwrap();
        assert_eq!(page.offset, offset);
        assert_eq!(page.total_bytes, original.len());
        assert_eq!(page.output, output);
        assert_eq!(page.turn, turn);
        combined.push_str(&page.text);
        let Some(next) = page.next_offset else {
            break;
        };
        assert!(next > offset && original.is_char_boundary(next));
        offset = next;
    }
    assert_eq!(combined, original);
}

#[test]
fn rejects_invalid_page_boundaries() {
    let value = serde_json::json!("中文");
    let reference = OutputReference {
        call: "call".into(),
        revision: blake3::hash(value.to_string().as_bytes())
            .to_hex()
            .to_string(),
    };
    let read = |offset, limit| {
        page(
            Resolved {
                session: SessionId::new(),
                turn: TurnId::new(),
                value: value.clone(),
            },
            reference.clone(),
            offset,
            limit,
        )
    };
    assert_eq!(read(2, 10).unwrap_err().code, ErrorCode::InvalidRequest);
    assert_eq!(read(0, 0).unwrap_err().code, ErrorCode::InvalidRequest);
    assert_eq!(read(100, 10).unwrap_err().code, ErrorCode::InvalidRequest);
    let first = read(1, 1).unwrap();
    assert_eq!(first.text, "中");
    assert_eq!(first.next_offset, Some(4));
}

#[tokio::test]
async fn checks_visible_history_and_invocation_owner() {
    use crate::store::agent::tests::fixture::Fixture;

    let mut fixture = Fixture::new().await;
    let source = fixture.session;
    let event = |call: &str, value: Value| {
        let mut event = AdkEvent::new("tool-output");
        event.set_content(adk_core::Content {
            role: "function".into(),
            parts: vec![adk_core::Part::FunctionResponse {
                function_response: adk_core::FunctionResponseData::new("read_file", value),
                id: Some(call.into()),
                annotations: None,
            }],
        });
        event
    };
    let value = serde_json::json!({"text": "Visible result"});
    let previous = serde_json::json!({"text":"Earlier result with the same call ID"});
    let previous_turn = fixture
        .append([event("visible-call", previous.clone())])
        .await;
    let previous_reference = OutputReference {
        call: "visible-call".into(),
        revision: blake3::hash(previous.to_string().as_bytes())
            .to_hex()
            .to_string(),
    };
    let reference = OutputReference {
        call: "visible-call".into(),
        revision: blake3::hash(value.to_string().as_bytes())
            .to_hex()
            .to_string(),
    };
    let visible = fixture
        .append([event(&reference.call, value.clone())])
        .await;
    let hidden = serde_json::json!({"text": "Outside the fork"});
    let hidden_reference = OutputReference {
        call: "hidden-call".into(),
        revision: blake3::hash(hidden.to_string().as_bytes())
            .to_hex()
            .to_string(),
    };
    fixture
        .append([event(&hidden_reference.call, hidden)])
        .await;
    let fork = fixture.fork(source, visible).await;
    fixture.session = fork.id;
    let turn = fixture.queued().await;
    crate::store::agent::runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    crate::store::agent::runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    let db = &fixture.database.connection;
    let caller = fixture.database.node;
    let result = resolve(db, caller, turn, &reference).await.unwrap();
    assert_eq!(result.session, fork.id);
    assert_eq!(result.turn, visible);
    assert_eq!(result.value, value);
    let earlier = resolve(db, caller, turn, &previous_reference)
        .await
        .unwrap();
    assert_eq!(earlier.turn, previous_turn);
    assert_eq!(earlier.value, previous);
    assert_eq!(
        resolve(db, caller, turn, &hidden_reference)
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        resolve(db, NodeId([44; 32]), turn, &reference)
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::PermissionDenied
    );
    let mut stale = reference;
    stale.revision = "0".repeat(64);
    assert_eq!(
        resolve(db, caller, turn, &stale).await.err().unwrap().code,
        ErrorCode::RevisionConflict
    );
}

#[tokio::test]
async fn hydrates_original_tool_json() {
    use crate::store::agent::tests::fixture::Fixture;
    let mut fixture = Fixture::new().await;
    let value = serde_json::json!({
        "content":[{"type":"image","mimeType":"image/png","data":"AQID"}],
        "structuredContent":{"revision":"original","nested":{"value":42}},
        "isError":false,
    });
    let mut event = AdkEvent::new("binary-output");
    event.set_content(adk_core::Content {
        role: "function".into(),
        parts: vec![adk_core::Part::FunctionResponse {
            id: Some("binary-call".into()),
            annotations: None,
            function_response: adk_core::FunctionResponseData::new("capture", value.clone()),
        }],
    });
    let turn = fixture.append([event]).await;
    let reference = OutputReference {
        call: "binary-call".into(),
        revision: blake3::hash(value.to_string().as_bytes())
            .to_hex()
            .to_string(),
    };
    let resolved =
        super::super::canonical::result(&fixture.database.connection, fixture.session, &reference)
            .await
            .unwrap();
    assert_eq!(resolved, (turn, value));
}
