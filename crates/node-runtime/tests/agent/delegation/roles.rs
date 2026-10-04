use super::*;

#[tokio::test]
async fn inherits_parent_model() {
    for remote in [false, true] {
        for named in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("project");
            std::fs::create_dir(&root).unwrap();
            let node = Node::start(directory.path().join("node")).await.unwrap();
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
            let key = named.then_some("review");
            let server = Server::parallel(vec![(
                crate::agent_support::plugin_tool("delegation", "spawn_agent"),
                json!({"role": key, "task": "Inherited child task"}),
            )])
            .await;
            let (mut session, mut provider) = configured(&client, &server.endpoint, &root).await;
            provider.models = vec![model("fixture-a")];
            provider.default_model = "fixture-a".into();
            client
                .execute(client.prepare(Command::PutProvider {
                    provider: provider.clone(),
                    expected_revision: 1,
                }))
                .await
                .unwrap();
            if named {
                let role = role::Profile {
                    appearance: None,
                    id: RoleId::new(),
                    revision: 0,
                    key: "review".into(),
                    name: "Review".into(),
                    description: String::new(),
                    model: None,
                    max_turns: Some(2),
                    skills: vec![],
                    instructions: "Inherited role instruction".into(),
                };
                let Output::Role(role) = client
                    .execute(client.prepare(Command::PutRole {
                        role,
                        expected_revision: 0,
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("role expected")
                };
                let Output::Session(updated) = client
                    .execute(client.prepare(Command::SetSessionRoles {
                        session: session.id,
                        expected_revision: 1,
                        roles: vec![role.reference()],
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("session expected")
                };
                session = updated;
            }
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::QueueTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: "Parent-only private context".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            provider.endpoint = "http://127.0.0.1:1/v1".into();
            client
                .execute(client.prepare(Command::PutProvider {
                    provider,
                    expected_revision: 2,
                }))
                .await
                .unwrap();
            client
                .execute(client.prepare(Command::StartQueuedTurn { turn: turn.id }))
                .await
                .unwrap();
            let page = finished(&client, session.id, turn.id).await;
            let output = results(&page);
            assert_eq!(output[0]["status"], "completed", "{output:?}");
            assert_eq!(output[0]["response"], "answer-fixture-a");
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 3);
            if named {
                let tool = requests[0]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|tool| {
                        tool["function"]["name"]
                            == crate::agent_support::plugin_tool("delegation", "spawn_agent")
                    })
                    .unwrap();
                assert_eq!(
                    tool["function"]["parameters"]["properties"]["role"]["enum"],
                    json!(["review"])
                );
                assert!(
                    requests[0]["messages"]
                        .to_string()
                        .contains("review: Review")
                );
            }
            let child = requests
                .iter()
                .find(|request| {
                    request["tools"].as_array().unwrap().iter().all(|tool| {
                        tool["function"]["name"]
                            != crate::agent_support::plugin_tool("delegation", "spawn_agent")
                    })
                })
                .unwrap();
            assert_eq!(child["reasoning_effort"], "high");
            assert!(
                !child["messages"]
                    .to_string()
                    .contains("Parent-only private context")
            );
            assert_eq!(
                child["messages"]
                    .to_string()
                    .contains("Inherited role instruction"),
                named
            );
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn validates_role_requirements() {
    for remote in [false, true] {
        for skills in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("project");
            std::fs::create_dir(&root).unwrap();
            let node = Node::start(directory.path().join("node")).await.unwrap();
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
            let parent = Server::parallel(vec![(
                crate::agent_support::plugin_tool("delegation", "spawn_agent"),
                json!({"role": "review", "task": "Bounded child task"}),
            )])
            .await;
            let child = Server::tools(vec![(
                plugin_tool("files", "list_directory"),
                json!({"path": ""}),
            )])
            .await;
            let (session, _, mut role) = setup(&client, &root, &parent, &child).await;
            role.max_turns = Some(1);
            if skills {
                role.skills.push("unconnected-skill".into());
            }
            let revision = role.revision;
            let Output::Role(role) = client
                .execute(client.prepare(Command::PutRole {
                    role,
                    expected_revision: revision,
                }))
                .await
                .unwrap()
            else {
                panic!("role expected")
            };
            client
                .execute(client.prepare(Command::SetSessionRoles {
                    session: session.id,
                    expected_revision: 2,
                    roles: vec![role.reference()],
                }))
                .await
                .unwrap();
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: 3,
                    message: "Use selected role".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let page = finished(&client, session.id, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Completed);
            let output = results(&page);
            assert!(output[0].get("error").is_some(), "{output:?}");
            if skills {
                assert_eq!(output[0]["error"]["code"], "not_configured");
                assert_eq!(child.requests.lock().unwrap().len(), 0);
                assert!(children(&client, session.id, 0).await.is_empty());
            } else {
                assert_eq!(child.requests.lock().unwrap().len(), 1);
                let children = children(&client, session.id, 1).await;
                let page = history(&client, children[0].id).await;
                assert_eq!(page.runs[0].status, Status::Failed);
                assert_eq!(results(&page).len(), 1);
                assert!(
                    !page.runs[0]
                        .error
                        .as_ref()
                        .unwrap()
                        .message
                        .contains("http")
                );
            }
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
        }
    }
}
