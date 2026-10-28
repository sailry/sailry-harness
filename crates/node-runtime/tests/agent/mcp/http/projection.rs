use super::*;

fn projected(request: &Value) -> &str {
    let message = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("MCP result must reach the model");
    message["content"].as_str().unwrap()
}

#[tokio::test]
async fn deduplicates_requests_and_preserves_history() {
    for remote in [false, true] {
        let http = Http::start(Reply::RepeatedJson).await;
        let name = alias("remote", "read");
        let text = "Complete result 中文 🙂\n".repeat(100);
        let output = json!({"value":text});
        let model = Server::tools(vec![(name.clone(), json!({"value":text}))]).await;
        let mut fixture = process::Fixture::packages(remote, &model, &[]).await;
        package(
            &fixture.root,
            json!({"remote":{"type":"streamable-http","url":http.endpoint}}),
            "repeated output",
        );
        install(&fixture, 0).await;
        let (_, turn) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert!(page.approvals.is_empty());
        let canonical = results(&fixture.client, &page).await;
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0]["structuredContent"], output);
        assert_eq!(
            canonical[0]["content"],
            json!([
                {"type":"text","text":output.to_string()},
                {"type":"text","text":"Distinct report note"}
            ])
        );
        let expected = format!("{output} Distinct report note");
        assert_eq!(projected(&model.requests.lock().unwrap()[1]), expected);
        assert_eq!(http.count("tools/call"), 1);

        fixture.node.shutdown().await.unwrap();
        fixture.node = Node::start_empty(fixture.directory.path().join("node"))
            .await
            .unwrap();
        fixture.client = Client::new(if remote {
            fixture
                .controller
                .handle()
                .remote(fixture.node.link().address())
        } else {
            fixture.node.local()
        });
        assert_eq!(history(&fixture.client, fixture.session.id).await, page);
        let (_, turn) = submit(&fixture, false).await;
        let restored = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(restored.runs.last().unwrap().status, Status::Completed);
        assert_eq!(results(&fixture.client, &restored).await, canonical);
        assert_eq!(
            projected(model.requests.lock().unwrap().last().unwrap()),
            expected
        );
        assert_eq!(http.count("tools/call"), 1);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
