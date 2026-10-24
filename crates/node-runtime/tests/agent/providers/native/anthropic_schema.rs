use super::*;
use serde_json::json;

#[tokio::test]
async fn preserves_management_branches_on_wire() {
    for remote in [false, true] {
        for mode in [WorkMode::Code, WorkMode::Plan] {
            let server = Server::start(ModelApi::Anthropic, Reply::Text).await;
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
                assert_eq!(requests.len(), 1);
                for name in ["manage_mcp", "manage_plugins", "manage_skills"] {
                    let tool = requests[0].body["tools"]
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
                    let plan = mode == WorkMode::Plan;
                    let cases = match name {
                        "manage_mcp" => vec![
                            (json!({"action":"list"}), true),
                            (
                                json!({"action":"uninstall","name":"mcp-fixture","expected_revision":1}),
                                !plan,
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
                                !plan,
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
                                !plan,
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
