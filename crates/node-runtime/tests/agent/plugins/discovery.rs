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
    restores_selection("tool_search", json!({"query":"manage_plugins","limit":1})).await;
}

#[tokio::test]
async fn remembers_direct_calls_without_discovery() {
    restores_selection("manage_plugins", json!({"action":"list"})).await;
}

async fn restores_selection(name: &str, arguments: Value) {
    for remote in [false, true] {
        let server = Server::tools(vec![(name.into(), arguments.clone())]).await;
        let mut fixture = process::Fixture::packages(remote, &server, &["files"]).await;
        if name == "manage_plugins" {
            planning::configure(&mut fixture, sailry_protocol::WorkMode::Plan).await;
        }
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn)
                .unwrap()
                .status,
            Status::Completed
        );
        if name == "manage_plugins" {
            let result = tool_details(&fixture.client, &page)
                .await
                .into_iter()
                .find(|(tool, _)| tool == name)
                .expect("direct management call")
                .1
                .result
                .expect("direct management result");
            assert!(result["plugins"].is_array(), "{result:?}");
        }
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        let first = &requests[0];
        let second = &requests[1];
        assert!(names(first).contains(&"tool_search"));
        assert!(names(first).contains(&"read_file"));
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
            &second["messages"].as_array().unwrap()[..prefix.len() - 1],
            &prefix[..prefix.len() - 1]
        );
        let context = turn_context(first).expect("initial request context expected");
        assert_eq!(
            message_text(&prefix.last().unwrap()["content"])
                .strip_suffix(context)
                .unwrap()
                .trim_end_matches('\n'),
            message_text(&second["messages"][prefix.len() - 1]["content"])
        );
        assert!(turn_context(second).is_some());

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
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn)
                .unwrap()
                .status,
            Status::Completed
        );
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            assert_eq!(requests[2]["tools"], second["tools"]);
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

async fn submit(fixture: &process::Fixture) -> TurnId {
    let Output::QueuedTurn(turn) = fixture
        .client
        .execute(fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Inspect the available capabilities".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn.id
}
