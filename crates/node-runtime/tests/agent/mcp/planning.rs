use super::*;

#[tokio::test]
async fn exposes_readonly_tools() {
    for remote in [false, true] {
        let model = Server::tools(vec![(alias("native", "read"), json!({}))]).await;
        let mut fixture = process::Fixture::new(remote, &model).await;
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
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert!(page.approvals.is_empty());
        assert_eq!(results(&page)[0]["output"]["version"], "planning fixture");
        let data = fixture.node.profile().join("plugins/data/example");
        assert!(!data.join("effects").exists());
        let requests = model.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        for request in requests {
            let names: Vec<_> = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["function"]["name"].as_str().unwrap())
                .collect();
            assert!(names.contains(&alias("native", "read").as_str()));
            for name in ["write", "fail", "hold"] {
                assert!(!names.contains(&alias("native", name).as_str()));
            }
        }
        fixture.node.shutdown().await.unwrap();
        reaped(&data);
        fixture.controller.close().await.unwrap();
    }
}
