use super::*;

#[tokio::test]
async fn preserves_readonly_execution() {
    for remote in [false, true] {
        let model = Server::scripted_tools(vec![
            (
                "tool_search".into(),
                json!({"query":"mcp_example_native","limit":4}),
            ),
            (alias("native", "read"), json!({})),
        ])
        .await;
        let mut fixture = process::Fixture::packages(remote, &model, &[]).await;
        package(
            &fixture.root,
            json!({
                "native": peer::config("mcp::peer::stdio_peer", "normal")
            }),
            "planning fixture",
        );
        install(&fixture, 0).await;

        super::super::planning::configure(&mut fixture, WorkMode::Plan).await;
        let (_, turn) = submit(&fixture, false).await;
        let page = questions::finish_plan(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert!(page.approvals.is_empty());
        let results = results(&fixture.client, &page).await;
        assert_eq!(results.len(), 2);
        let expected: std::collections::BTreeSet<_> = ["read", "write", "fail", "hold"]
            .into_iter()
            .map(|name| alias("native", name))
            .collect();
        assert_eq!(
            results[0]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["name"].as_str().unwrap().to_owned())
                .collect::<std::collections::BTreeSet<_>>(),
            expected
        );
        assert_eq!(
            results[1]["structuredContent"]["version"],
            "planning fixture"
        );
        let data = fixture.node.profile().join("plugins/data/example");
        assert!(!data.join("effects").exists());
        let requests = model.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        let initial = requests[0]["tools"].as_array().unwrap();
        assert!(
            initial
                .iter()
                .any(|tool| tool["function"]["name"] == "tool_search")
        );
        assert!(
            initial
                .iter()
                .all(|tool| !expected.contains(tool["function"]["name"].as_str().unwrap()))
        );
        for request in &requests[1..] {
            let names: Vec<_> = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["function"]["name"].as_str().unwrap())
                .collect();
            assert!(names.contains(&alias("native", "read").as_str()));
            for name in ["write", "fail", "hold"] {
                assert!(names.contains(&alias("native", name).as_str()));
            }
        }
        assert_eq!(requests[1]["tools"], requests[2]["tools"]);
        model.replace_tools(vec![(
            alias("native", "write"),
            json!({"value":"must not run"}),
        )]);
        let (_, denied) = submit(&fixture, false).await;
        let denied_page = finished(&fixture.client, fixture.session.id, denied.id).await;
        planning_denial(&denied_page, denied.id);
        let denied_requests = model.requests.lock().unwrap().clone();
        assert_eq!(denied_requests.len(), 4);
        assert_eq!(denied_requests[3]["tools"], requests[2]["tools"]);
        assert!(!data.join("effects").exists());
        assert!(!data.join("holding").exists());
        fixture.node.shutdown().await.unwrap();
        reaped(&data);
        fixture.controller.close().await.unwrap();
    }
}
