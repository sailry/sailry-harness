use super::*;
use sailry_protocol::tool::Presentation;

#[tokio::test]
async fn preserves_failures_without_changing_returns() {
    let model = Server::batch(vec![
        (
            "set_session_title".into(),
            json!({"title":"Presentation fixture"}),
        ),
        ("set_session_title".into(), json!({"title":"\n"})),
        (alias("native", "read"), json!({"value":"failure"})),
        (alias("native", "read"), json!({"value":"business"})),
        (alias("native", "read"), json!({"value":"business_flags"})),
        ("list_sessions".into(), json!({})),
    ])
    .await;
    let fixture = process::Fixture::packages(&model, &[]).await;
    package(
        &fixture.root,
        json!({"native":peer::config("mcp::peer::stdio_peer","presentation")}),
        "0.1.0",
    );
    install(&fixture, 0).await;
    let (_, turn) = submit(&fixture, false).await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
    let view = super::super::progress::observe(&fixture.client, fixture.session.id).await;
    assert_eq!(view.calls.len(), 6);
    let mut calls = Vec::new();
    for call in view.calls.iter() {
        let detail = crate::agent_support::details::read(
            &fixture.client,
            call.detail_reference(&page).unwrap(),
        )
        .await
        .unwrap();
        calls.push(call.with_detail(detail));
    }
    assert_eq!(calls[0].id.as_deref(), Some("call-fixture-0"));
    assert_eq!(calls[1].id.as_deref(), Some("call-fixture-1"));
    assert_eq!(calls[0].presentation, Presentation::Summary);
    assert!(!calls[0].hidden(&page));
    assert!(!calls[0].details(&page));
    assert!(calls[1].failure.is_some());
    assert!(!calls[1].hidden(&page));
    assert!(!calls[1].details(&page));
    assert_eq!(calls[1].result(&page).unwrap()["code"], -32603);
    assert_eq!(
        calls[1].result(&page).unwrap()["data"]["code"],
        "invalid_request"
    );
    for call in &calls[2..] {
        assert_eq!(call.presentation, Presentation::Summary);
    }
    assert!(calls[2].failure.is_some());
    assert!(!calls[2].details(&page));
    let failure = calls[2].result(&page).unwrap();
    assert_eq!(failure["isError"], true);
    assert_eq!(
        failure["content"],
        json!([{"type":"text","text":"Original MCP failure\n参数错误 🙂"}])
    );
    assert!(calls[3].failure.is_none());
    assert!(!calls[3].details(&page));
    assert!(calls[3].raw_result(&page));
    let business = json!({"error":"business value","id":"business-id","node":[119,14,79],"text":"Literal 内容 🙂"});
    assert_eq!(
        calls[3].result(&page).unwrap()["structuredContent"],
        business
    );
    assert!(calls[4].failure.is_none());
    assert!(!calls[4].diagnostics(&page));
    assert!(!calls[4].details(&page));
    assert!(calls[4].raw_result(&page));
    assert_eq!(
        calls[4].result(&page).unwrap()["structuredContent"],
        json!({"isError":true,"error":"domain row"})
    );
    assert_eq!(calls[4].result(&page).unwrap()["isError"], false);
    assert!(!calls[5].details(&page));
    assert!(calls[5].resolved().unwrap().label.is_some());
    assert_eq!(calls[5].result(&page).unwrap()["sessions"], json!([]));
    {
        let requests = model.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        for (index, call) in calls.iter().enumerate() {
            let message = requests[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| {
                    message["role"] == "tool"
                        && message["tool_call_id"].as_str() == call.id.as_deref()
                })
                .unwrap();
            let raw = call.result(&page).unwrap();
            let expected = match index {
                0 | 5 => raw.to_string(),
                1 => format!(
                    "The tool call returned the following error:\n{}",
                    serde_json::from_value::<rmcp::model::ErrorData>(raw.clone()).unwrap()
                ),
                2 => "Original MCP failure\n参数错误 🙂".into(),
                3 | 4 => "(empty result)".into(),
                _ => unreachable!(),
            };
            assert_eq!(message["content"], expected);
        }
    }
    let path = fixture.node.profile().to_owned();
    fixture.node.shutdown().await.unwrap();
    let node = Node::start_empty(&path).await.unwrap();
    let client = Client::new(fixture.controller.handle().remote(node.link().address()));
    assert_eq!(history(&client, fixture.session.id).await, page);
    let restored = super::super::progress::observe(&client, fixture.session.id).await;
    assert_eq!(restored.calls[1].failure, calls[1].failure);
    assert_eq!(restored.calls[2].failure, calls[2].failure);
    assert!(!restored.calls[0].hidden(&page));
    assert!(!restored.calls[3].details(&page));
    assert!(restored.calls[4].failure.is_none());
    assert!(!restored.calls[4].details(&page));
    node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn classifies_server_errors_without_changing_payloads() {
    use sailry_protocol::tool::Failure;
    let model = Server::batch(
        ["failure_json", "rpc_error", "rpc_fault"]
            .into_iter()
            .map(|value| (alias("native", "read"), json!({"value":value})))
            .collect(),
    )
    .await;
    let fixture = process::Fixture::packages(&model, &[]).await;
    package(
        &fixture.root,
        json!({"native":peer::config("mcp::peer::stdio_peer","presentation")}),
        "0.1.0",
    );
    install(&fixture, 0).await;
    let (_, turn) = submit(&fixture, false).await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(page.runs.last().unwrap().status, Status::Completed);
    let view = super::super::progress::observe(&fixture.client, fixture.session.id).await;
    assert_eq!(view.calls.len(), 3);
    let raw = results(&fixture.client, &page).await;
    let expected = [
        (
            Failure::tool(None),
            "No running application matches 'Terminal'",
        ),
        (
            Failure::tool(None),
            "No running application matches 'Terminal'",
        ),
        (
            Failure::runtime(Some(ErrorCode::Unavailable)),
            "Sailry service is unavailable",
        ),
    ];
    for (index, (call, (failure, message))) in view.calls.iter().zip(expected).enumerate() {
        assert_eq!(call.failure.as_ref(), Some(&failure));
        assert_eq!(call.summary(&page).unwrap().error.as_deref(), Some(message));
        assert!(!call.summary(&page).unwrap().has_details);
        let row = call.row(&page);
        assert_eq!(row.failed, index == 2);
        assert_eq!(row.warning, index != 2);
        if index != 2 {
            assert!(row.state.is_none());
        }
    }
    assert_eq!(raw[0]["isError"], true);
    assert!(
        raw[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("APP_NOT_FOUND")
    );
    for result in &raw[1..] {
        assert_eq!(result["data"]["code"], "outcome_unknown");
        assert!(
            result["data"]["message"]
                .as_str()
                .unwrap()
                .starts_with("MCP call failed without replay; remote effects may be uncertain: ")
        );
    }
    {
        let requests = model.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        for (index, result) in raw.iter().enumerate() {
            let id = format!("call-fixture-{index}");
            let message = requests[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| message["role"] == "tool" && message["tool_call_id"] == id)
                .unwrap();
            let expected = if index == 0 {
                result["content"][0]["text"].as_str().unwrap().to_owned()
            } else {
                format!(
                    "The tool call returned the following error:\n{}",
                    serde_json::from_value::<rmcp::model::ErrorData>(result.clone()).unwrap()
                )
            };
            assert_eq!(message["content"], expected);
        }
    }
    let Output::Snapshot(snapshot) = execute(&fixture.client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    assert!(
        !snapshot
            .notifications
            .iter()
            .any(|notice| notice.content.kind == notification::Kind::Error
                && notice.content.session == Some(fixture.session.id))
    );
    let profile = fixture.node.profile().to_owned();
    fixture.node.shutdown().await.unwrap();
    let node = Node::start_empty(profile).await.unwrap();
    let client = Client::new(fixture.controller.handle().remote(node.link().address()));
    assert_eq!(history(&client, fixture.session.id).await, page);
    node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}
