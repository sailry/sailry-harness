use super::*;
use adk_core::SchemaAdapter;
use adk_model::openai::OpenAiSchemaAdapter;

fn parameters(definition: &Value) -> Value {
    let mut schema = definition["inputSchema"].clone();
    schema["properties"]
        .as_object_mut()
        .unwrap()
        .remove("session");
    if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|field| field != "session");
    }
    schema
}

#[tokio::test]
async fn preserves_host_bound_native_definitions() {
    let definitions = driver::catalog();
    assert!(
        definitions
            .iter()
            .any(|tool| tool["name"] == "get_window_state")
    );
    assert!(definitions.iter().any(|tool| tool["name"] == "click"));
    for remote in [false, true] {
        for enabled in [false, true] {
            for mode in [WorkMode::Code, WorkMode::Plan] {
                let fixture =
                    Fixture::new(remote, vec![], mode, Permission::Full, enabled, json!({})).await;
                let page = finished(&fixture.client, fixture.session, fixture.turn).await;
                assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                {
                    let requests = fixture.server.requests.lock().unwrap();
                    let tools = requests[0]["tools"].as_array().unwrap();
                    for definition in &definitions {
                        let name = definition["name"].as_str().unwrap();
                        let expected = enabled
                            && (mode == WorkMode::Code
                                || definition["annotations"]["readOnlyHint"] == true);
                        let offered = tools.iter().find(|tool| tool["function"]["name"] == name);
                        assert_eq!(
                            offered.is_some(),
                            expected,
                            "{name}, mode={mode:?}, remote={remote}"
                        );
                        if let Some(offered) = offered {
                            assert_eq!(
                                offered["function"]["description"], definition["description"],
                                "{name}"
                            );
                            assert_eq!(
                                offered["function"]["parameters"],
                                OpenAiSchemaAdapter.normalize_schema(parameters(definition)),
                                "{name}"
                            );
                            assert!(
                                offered["function"]["parameters"]["properties"]
                                    .get("session")
                                    .is_none(),
                                "{name}"
                            );
                        }
                    }
                    assert!(!tools.iter().any(|tool| {
                        tool["function"]["name"]
                            .as_str()
                            .is_some_and(|name| name.contains("computer_"))
                    }));
                    assert!(
                        !requests[0]["messages"]
                            .to_string()
                            .contains("# Computer operation workflow")
                    );
                    let original = definitions
                        .iter()
                        .find(|tool| tool["name"] == "list_apps")
                        .unwrap();
                    assert!(
                        original["inputSchema"]["properties"]
                            .get("session")
                            .is_some()
                    );
                }
                assert!(
                    fixture.worker.records().is_empty(),
                    "catalog inspection must not start a worker"
                );
                fixture.shutdown().await;
            }
        }
    }
}

#[tokio::test]
async fn responses_preserves_optional_parameters() {
    let definitions = driver::catalog();
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let worker = driver::Worker::new(directory.path(), json!({}));
        let node = Node::start_with_computer(
            directory.path().join("node"),
            NetworkScope::default(),
            Some(worker.configuration.clone()),
        )
        .await
        .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let server = Server::web_search().await;
        let session = super::super::approvals::prepare(&client, &server, &root).await;
        let Output::Providers(mut providers) = client
            .execute(client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        let mut provider = providers.remove(0);
        provider.api = ModelApi::Responses;
        provider.models[0].vision = true;
        client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        // Keep this fixture about declarations, not execution of other packages.
        disable_tools(&client, &["files"]).await;
        set_enabled(&client, true).await;
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Inspect tool declarations".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            let tools = requests[0]["tools"].as_array().unwrap();
            for definition in &definitions {
                let name = definition["name"].as_str().unwrap();
                let offered = tools.iter().find(|tool| tool["name"] == name).unwrap();
                assert_eq!(offered["type"], "function", "{name}");
                assert_eq!(offered["strict"], false, "{name}");
                assert_eq!(offered["description"], definition["description"], "{name}");
                assert_eq!(offered["parameters"], parameters(definition), "{name}");
            }
            let window = tools
                .iter()
                .find(|tool| tool["name"] == "get_window_state")
                .unwrap();
            let required = window["parameters"]["required"].as_array().unwrap();
            assert!(!required.iter().any(|field| field == "screenshot_out_file"));
            assert!(window["parameters"]["properties"]["screenshot_out_file"].is_object());
            assert!(window["parameters"]["properties"].get("session").is_none());
        }
        assert!(worker.records().is_empty());
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
