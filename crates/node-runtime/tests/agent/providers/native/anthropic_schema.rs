use super::*;
use serde_json::json;

fn discovery() -> Reply {
    let scripts = [true, false].map(|search| {
        let block = if search {
            json!({"type":"tool_use","id":"management-search","name":"tool_search","input":{}})
        } else {
            json!({"type":"text","text":""})
        };
        let delta = if search {
            json!({"type":"input_json_delta","partial_json":json!({"query":"manage","limit":8}).to_string()})
        } else {
            json!({"type":"text_delta","text":server::ANSWER})
        };
        let events = [
            json!({"type":"message_start","message":{"id":"management-answer","type":"message","role":"assistant","model":"fixture-a","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":0}}}),
            json!({"type":"content_block_start","index":0,"content_block":block}),
            json!({"type":"content_block_delta","index":0,"delta":delta}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":if search {"tool_use"} else {"end_turn"},"stop_sequence":null},"usage":{"output_tokens":4}}),
            json!({"type":"message_stop"}),
        ];
        let mut body = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n".to_string();
        for event in events {
            body.push_str(&format!("event: {}\ndata: {event}\n\n", event["type"].as_str().unwrap()));
        }
        server::Script { bytes: body.into_bytes(), ending: server::Ending::Close }
    });
    Reply::Scripted(Arc::new(scripts.into()))
}

#[tokio::test]
async fn preserves_management_branches_on_wire() {
    for remote in [false, true] {
        for mode in [WorkMode::Code, WorkMode::Plan] {
            let server = Server::start(ModelApi::Anthropic, discovery()).await;
            let mut fixture = Fixture::empty(remote, ModelApi::Anthropic, &server.endpoint).await;
            let mut config = fixture.session.config.clone();
            config.mode = mode;
            let Output::Session(session) = execute(
                &fixture.client,
                Command::SetSessionConfig {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    config,
                },
            )
            .await
            else {
                panic!("session expected")
            };
            fixture.session = session;
            let turn = fixture.submit("Say hello briefly").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            {
                let requests = server.requests.lock().unwrap();
                assert_eq!(requests.len(), 2);
                let initial = requests[0].body["tools"].as_array().unwrap();
                assert!(initial.iter().any(|tool| tool["name"] == "tool_search"));
                for name in ["manage_mcp", "manage_plugins", "manage_skills"] {
                    assert!(!initial.iter().any(|tool| tool["name"] == name));
                    let tool = requests[1].body["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|tool| tool["name"] == name)
                        .unwrap();
                    let schema = &tool["input_schema"];
                    assert_eq!(schema["type"], "object");
                    assert_eq!(schema["required"], json!(["action"]));
                    assert!(schema["properties"]["action"]["enum"].is_array());
                    for keyword in ["oneOf", "allOf", "anyOf"] {
                        assert!(schema.get(keyword).is_none(), "{name}: {keyword}");
                    }
                    assert!(schema["not"]["not"]["oneOf"].is_array());
                    let cases = match name {
                        "manage_mcp" => vec![
                            (json!({"action":"list"}), true),
                            (
                                json!({"action":"uninstall","name":"mcp-fixture","expected_revision":1}),
                                true,
                            ),
                            (json!({"action":"uninstall","name":"mcp-fixture"}), false),
                            (json!({"action":"list","name":"mcp-fixture"}), false),
                            (json!({"action":"install"}), false),
                            (json!({"action":"unknown"}), false),
                        ],
                        "manage_plugins" => vec![
                            (json!({"action":"list"}), true),
                            (json!({"action":"search","source":"official"}), true),
                            (
                                json!({"action":"uninstall","name":"fixture","expected_revision":1}),
                                true,
                            ),
                            (json!({"action":"search"}), false),
                            (json!({"action":"uninstall","name":"fixture"}), false),
                            (json!({"action":"list","name":"fixture"}), false),
                            (
                                json!({"action":"inspect","target":{"kind":"installed"}}),
                                false,
                            ),
                        ],
                        "manage_skills" => vec![
                            (json!({"action":"list"}), true),
                            (json!({"action":"discover","path":"skills"}), true),
                            (
                                json!({"action":"uninstall","name":"fixture","expected_revision":1}),
                                true,
                            ),
                            (
                                json!({"action":"discover","path":"skills","repository":"https://example.invalid/repo"}),
                                true,
                            ),
                            (
                                json!({"action":"discover","repository":"https://example.invalid/repo","registry":{"slug":"fixture"}}),
                                false,
                            ),
                            (json!({"action":"uninstall","name":"fixture"}), false),
                            (json!({"action":"list","name":"fixture"}), false),
                        ],
                        _ => unreachable!(),
                    };
                    let projected = jsonschema::draft202012::options().build(schema).unwrap();
                    for (arguments, valid) in cases {
                        assert_eq!(
                            projected.is_valid(&arguments),
                            valid,
                            "{name}, {mode:?}, remote={remote}: {arguments}"
                        );
                    }
                }
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}
