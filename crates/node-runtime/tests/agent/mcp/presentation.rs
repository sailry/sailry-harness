use super::*;
use sailry_protocol::tool::Presentation;

#[tokio::test]
async fn preserves_failures_without_changing_returns() {
    for remote in [false, true] {
        let model = Server::batch(vec![
            (
                "set_session_title".into(),
                json!({"title":"Presentation fixture"}),
            ),
            ("set_session_title".into(), json!({"title":"\n"})),
            (alias("native", "read"), json!({"value":"failure"})),
            (alias("native", "read"), json!({"value":"business"})),
            ("list_sessions".into(), json!({})),
        ])
        .await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
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
        assert_eq!(view.calls.len(), 5);
        let calls = &view.calls;
        assert_eq!(calls[0].id.as_deref(), Some("call-fixture-0"));
        assert_eq!(calls[1].id.as_deref(), Some("call-fixture-1"));
        assert_eq!(calls[0].presentation, Presentation::Hidden);
        assert!(calls[0].hidden(&page));
        assert!(!calls[0].details(&page));
        assert!(calls[1].failure.is_some());
        assert!(!calls[1].hidden(&page));
        assert!(calls[1].details(&page));
        assert!(calls[1].result(&page).unwrap()["error"].is_string());
        for call in &calls[2..] {
            assert_eq!(call.presentation, Presentation::Summary);
        }
        assert!(calls[2].failure.is_some());
        assert!(calls[2].details(&page));
        assert_eq!(
            calls[2].result(&page),
            Some(&json!({"error":"Original MCP failure\n参数错误 🙂"}))
        );
        assert!(calls[3].failure.is_none());
        assert!(!calls[3].details(&page));
        assert!(calls[3].raw_result(&page));
        let business = json!({"error":"business value","id":"business-id","node":[119,14,79],"text":"Literal 内容 🙂"});
        assert_eq!(calls[3].result(&page).unwrap()["output"], business);
        assert!(!calls[4].details(&page));
        assert!(calls[4].display(&page).unwrap().valid());
        assert_eq!(calls[4].result(&page).unwrap()["sessions"], json!([]));
        {
            let requests = model.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            for call in calls.iter() {
                let message = requests[1]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|message| {
                        message["role"] == "tool"
                            && message["tool_call_id"].as_str() == call.id.as_deref()
                    })
                    .unwrap();
                let result: Value =
                    serde_json::from_str(message["content"].as_str().unwrap()).unwrap();
                assert_eq!(&result, call.result(&page).unwrap());
            }
        }
        let path = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty(&path).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&client, fixture.session.id).await, page);
        let restored = super::super::progress::observe(&client, fixture.session.id).await;
        assert_eq!(restored.calls[1].failure, calls[1].failure);
        assert_eq!(restored.calls[2].failure, calls[2].failure);
        assert!(restored.calls[0].hidden(&page));
        assert!(!restored.calls[3].details(&page));
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
