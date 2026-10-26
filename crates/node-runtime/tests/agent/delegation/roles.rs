use super::*;

#[path = "roles/defaults.rs"]
mod defaults;

#[tokio::test]
async fn inherits_parent_model() {
    for remote in [false, true] {
        for (roster, selection) in [
            (false, None),
            (false, Some(None)),
            (true, None),
            (true, Some(None)),
            (true, Some(Some("review"))),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("project");
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("AGENTS.md"), "Shared default instructions").unwrap();
            let node = Node::start_empty(directory.path().join("node"))
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
            let mut args = json!({"task": "第一轮：读取并总结"});
            if let Some(role) = selection {
                args["role"] = json!(role);
            }
            let server = Server::parallel(vec![(
                crate::agent_support::plugin_tool("delegation", "spawn_agent"),
                args,
            )])
            .await;
            install(&client).await;
            let (mut session, mut provider) =
                configured_session(&client, &server.endpoint, &root).await;
            provider.models = vec![model("fixture-a")];
            provider.default_model = "fixture-a".into();
            client
                .execute(client.prepare(Command::PutProvider {
                    provider: provider.clone(),
                    expected_revision: 1,
                }))
                .await
                .unwrap();
            if roster {
                let role = role::Profile {
                    read_only: false,
                    appearance: None,
                    id: RoleId::new(),
                    revision: 0,
                    key: "review".into(),
                    name: "Review".into(),
                    description: String::new(),
                    model: None,
                    max_turns: Some(2),
                    skills: vec![],
                    tools: None,
                    instructions: format!(
                        "Inherited role instruction {}Full child instruction tail",
                        "🙂".repeat(1024)
                    ),
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
            } else {
                // Empty-roster behavior remains explicit after adding a shipped preset.
                let Output::Session(updated) = client
                    .execute(client.prepare(Command::SetSessionRoles {
                        session: session.id,
                        expected_revision: session.revision,
                        roles: vec![],
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
            let parent_instruction = requests[0]["messages"][0].to_string();
            assert!(!parent_instruction.contains("Choose a role from the admitted roster"));
            let declaration = requests[0]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| {
                    tool["function"]["name"]
                        == crate::agent_support::plugin_tool("delegation", "spawn_agent")
                })
                .unwrap();
            let description = declaration["function"]["description"].as_str().unwrap();
            assert!(description.contains("use its exact listed key and never invent a role"));
            assert!(
                description
                    .contains("If no role fits or the roster is empty, omit role or use null")
            );
            assert!(description.contains("The child does not see this conversation"));
            assert!(description.contains("self-contained task"));
            assert!(description.contains("disjoint files"));
            if roster {
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
                    json!(["review", null])
                );
                assert_eq!(tool["function"]["parameters"]["required"], json!(["task"]));
                assert_eq!(
                    tool["function"]["parameters"]["properties"]["role"]["type"],
                    json!(["string", "null"])
                );
                assert!(
                    requests[0]["messages"]
                        .to_string()
                        .contains("review (Review): Inherited role instruction")
                );
                assert!(!parent_instruction.contains("Full child instruction tail"));
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
            assert_eq!(child["model"], requests[0]["model"]);
            assert_eq!(child["messages"][0]["role"], "system");
            let identity = child["messages"][0].to_string();
            assert!(identity.contains("You are a delegated child"));
            assert!(
                identity.contains("Your final message returns to the parent agent, not the user")
            );
            assert!(identity.contains("Do not ask the user for input"));
            assert!(!identity.contains("Finish with an answer to the user's request"));
            assert!(!identity.contains("renders a file card"));
            for name in ["ask_user", "set_session_title"] {
                assert!(
                    child["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|tool| tool["function"]["name"] != name)
                );
            }

            assert!(
                child["messages"][0]
                    .to_string()
                    .contains("Shared default instructions")
            );
            assert!(child["messages"].to_string().contains("第一轮：读取并总结"));
            assert!(
                !child["messages"]
                    .to_string()
                    .contains("Parent-only private context")
            );
            assert_eq!(
                child["messages"]
                    .to_string()
                    .contains("Inherited role instruction"),
                selection == Some(Some("review"))
            );
            assert_eq!(
                child["messages"]
                    .to_string()
                    .contains("Full child instruction tail"),
                selection == Some(Some("review"))
            );
            let children = children(&client, session.id, 1).await;
            assert_eq!(children[0].config, turn.config);
            assert_eq!(
                children[0].delegation.as_ref().unwrap().role.is_some(),
                selection == Some(Some("review"))
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
            let node = Node::start_empty(directory.path().join("node"))
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
                role.skills.push("unconnected:skill".into());
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
            let view = super::super::progress::observe(&client, session.id).await;
            assert_eq!(view.calls.len(), 1);
            if skills {
                assert_eq!(
                    view.calls[0]
                        .failure
                        .as_ref()
                        .and_then(|failure| failure.code),
                    Some(ErrorCode::InvalidRequest)
                );
                assert_eq!(output[0]["error"]["code"], "invalid_request");
                assert_eq!(
                    output[0]["error"]["message"],
                    "skill is not listed in this turn's catalog"
                );
                assert_eq!(child.requests.lock().unwrap().len(), 0);
                assert!(children(&client, session.id, 0).await.is_empty());
            } else {
                assert!(view.calls[0].failure.is_none());
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
