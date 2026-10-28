use super::*;
use serde_json::{Value, json};

#[tokio::test]
async fn idless_mcp_results_keep_execution_facts() {
    use super::super::super::mcp::peer;
    for remote in [false, true] {
        let name = peer::alias("native", "read");
        let model_name = name.clone();
        let closed = Arc::new(tokio::sync::Notify::new());
        let waiting = closed.clone();
        let model = Server::start(
            ModelApi::Gemini,
            Reply::Dynamic(Arc::new(move |index| {
                if index >= 2 {
                    return server::Script {
                        bytes: b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n".to_vec(),
                        ending: server::Ending::Closed(waiting.clone()),
                    };
                }
                let parts = if index == 0 {
                    json!([
                        {"functionCall":{"name":model_name,"args":{"value":"failure"}}},
                        {"functionCall":{"name":model_name,"args":{"value":"business_flags"}}},
                        {"functionCall":{"name":model_name,"args":{"value":"large_first"}}},
                        {"functionCall":{"name":model_name,"args":{"value":"large_second"}}},
                    ])
                } else {
                    json!([{"text":"Observed tool results"}])
                };
                let event = json!({"candidates":[{"content":{"role":"model","parts":parts},"finishReason":"STOP","index":0}],
                    "usageMetadata":{"promptTokenCount":8,"candidatesTokenCount":4,"totalTokenCount":12},"modelVersion":"fixture-a"});
                server::Script {
                    bytes: format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {event}\n\n").into_bytes(),
                    ending: server::Ending::Close,
                }
            })),
        )
        .await;
        let fixture = Fixture::empty(remote, ModelApi::Gemini, &model.endpoint).await;
        let package = fixture.root.join("package");
        std::fs::create_dir(&package).unwrap();
        std::fs::write(package.join("plugin.json"), json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"example","version":"0.1.0"}).to_string()).unwrap();
        std::fs::write(package.join("mcp.json"), json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"native":peer::config("mcp::peer::stdio_peer","presentation")}}).to_string()).unwrap();
        execute(
            &fixture.client,
            Command::InstallPlugin {
                worktree: fixture.session.worktree,
                path: "package".into(),
                name: "example".into(),
                expected_revision: 0,
                expected_digest: None,
            },
        )
        .await;
        let turn = fixture.submit("Inspect the tool results").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        let view =
            super::super::super::progress::observe(&fixture.client, fixture.session.id).await;
        let mut results = Vec::new();
        for call in view.calls.iter().filter(|call| call.response.is_some()) {
            results.push(
                call.with_detail(
                    crate::agent_support::details::read(
                        &fixture.client,
                        call.detail_reference(&page).unwrap(),
                    )
                    .await
                    .unwrap(),
                ),
            );
        }
        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|call| {
            call.name == name && call.id.as_deref().is_some_and(|id| !id.is_empty())
        }));
        assert_ne!(results[0].id, results[1].id);
        let failed = results.iter().find(|call| call.failure.is_some()).unwrap();
        assert!(failed.diagnostics(&page));
        assert!(failed.details(&page));
        assert_eq!(
            failed.result(&page),
            Some(&json!({"error":"Original MCP failure\n参数错误 🙂"}))
        );
        let business = results
            .iter()
            .find(|call| {
                call.result(&page)
                    .is_some_and(|result| result["output"]["isError"] == true)
            })
            .unwrap();
        assert!(!business.diagnostics(&page));
        assert!(!business.details(&page));
        assert_eq!(
            business.result(&page),
            Some(&json!({"output":{"isError":true,"error":"domain row"}}))
        );
        let returned = {
            let requests = model.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(
                requests[1].body["contents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|content| content["parts"].as_array().into_iter().flatten())
                    .filter_map(|part| part.get("functionCall"))
                    .all(|call| call["id"].as_str().is_some_and(|id| !id.is_empty()))
            );
            let returned: Vec<_> = requests[1].body["contents"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|content| content["parts"].as_array().into_iter().flatten())
                .filter_map(|part| part.get("functionResponse").cloned())
                .collect();
            assert_eq!(returned.len(), 4);
            assert!(results.iter().all(|call| {
                returned
                    .iter()
                    .any(|response| response["id"].as_str() == call.id.as_deref())
            }));
            assert!(
                returned
                    .iter()
                    .all(|response| response["response"].get("sailry_output").is_none())
            );
            for call in results.iter().filter(|call| {
                call.result(&page)
                    .is_some_and(|result| result["output"]["label"].is_null())
            }) {
                assert!(
                    returned
                        .iter()
                        .any(|response| Some(&response["response"]) == call.result(&page))
                );
            }
            returned
        };
        let active = fixture.submit("Read the retained original results").await;
        tokio::time::timeout(Duration::from_secs(10), async {
            while model.requests.lock().unwrap().len() != 3 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("follow-up request deadline");
        for call in results.iter().filter(|call| {
            call.result(&page)
                .is_some_and(|result| result["output"]["label"].is_string())
        }) {
            let response = returned
                .iter()
                .find(|response| {
                    response["response"]["output"]["call"].as_str() == call.id.as_deref()
                })
                .unwrap();
            let preview = &response["response"];
            assert!(preview.to_string().len() < 40 * 1024);
            assert!(!preview.to_string().contains("sailry_output"));
            assert!(
                preview["text"]
                    .as_str()
                    .unwrap()
                    .contains("read_tool_output")
            );
            let reference: sailry_protocol::tool::OutputReference =
                serde_json::from_value(preview["output"].clone()).unwrap();
            let mut offset = 0;
            let mut original = String::new();
            loop {
                let Output::ToolOutput(output) = execute(
                    &fixture.client,
                    Command::ReadToolOutput {
                        turn: active,
                        output: reference.clone(),
                        stream: None,
                        offset,
                        limit: 4093,
                    },
                )
                .await
                else {
                    panic!("canonical MCP output expected")
                };
                assert_eq!(output.offset, offset);
                original.push_str(&output.text);
                match output.next_offset {
                    Some(next) => {
                        assert!(next > offset);
                        offset = next;
                    }
                    None => break,
                }
            }
            assert_eq!(
                reference.revision,
                blake3::hash(original.as_bytes()).to_hex().to_string()
            );
            let original: Value = serde_json::from_str(&original).unwrap();
            assert_eq!(Some(&original), call.result(&page));
            assert_eq!(original["text"].as_array().unwrap().len(), 1);
            assert_eq!(
                serde_json::from_str::<Value>(original["text"][0].as_str().unwrap()).unwrap(),
                original["output"]
            );
        }
        execute(&fixture.client, Command::StopTurn { turn: active }).await;
        let stopped = finished(&fixture.client, fixture.session.id, active).await;
        assert_eq!(stopped.runs.last().unwrap().status, Status::Cancelled);
        assert_eq!(&stopped.entries[..page.entries.len()], &page.entries);
        tokio::time::timeout(Duration::from_secs(10), closed.notified())
            .await
            .unwrap();
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&client, fixture.session.id).await, stopped);
        assert_eq!(model.requests.lock().unwrap().len(), 3);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
