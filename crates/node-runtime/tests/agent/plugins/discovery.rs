#![cfg(unix)]
use super::*;
use serde_json::{Value, json};

fn names(request: &Value) -> Vec<&str> {
    request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect()
}

#[tokio::test]
async fn loads_captured_tools_and_restores_selection() {
    for remote in [false, true] {
        let server = Server::tools(vec![(
            "tool_search".into(),
            json!({"query":"manage_plugins","limit":1}),
        )])
        .await;
        let mut fixture = process::Fixture::packages(remote, &server, &["files"]).await;
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn)
                .unwrap()
                .status,
            Status::Completed
        );
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        let first = &requests[0];
        let second = &requests[1];
        assert!(names(first).contains(&"tool_search"));
        assert!(names(first).contains(&agent_support::plugin_tool("files", "read_file").as_str()));
        assert!(!names(first).contains(&"manage_plugins"));
        assert!(names(second).contains(&"manage_plugins"));
        let unchanged: Vec<_> = second["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|tool| tool["function"]["name"] != "manage_plugins")
            .collect();
        assert_eq!(serde_json::to_value(unchanged).unwrap(), first["tools"]);
        let prefix = first["messages"].as_array().unwrap();
        assert_eq!(
            &second["messages"].as_array().unwrap()[..prefix.len()],
            prefix
        );

        let node = fixture.node.id();
        fixture.node.shutdown().await.unwrap();
        fixture.controller.handle().disconnect(node).await;
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
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn)
                .unwrap()
                .status,
            Status::Completed
        );
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[2]["tools"], second["tools"]);
        drop(requests);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
