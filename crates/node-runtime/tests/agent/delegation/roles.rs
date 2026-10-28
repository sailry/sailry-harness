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
            let mut config = session.config.clone();
            config
                .options
                .entry("delegation".into())
                .or_default()
                .insert("mode".into(), json!("multitask"));
            let Output::Session(updated) = client
                .execute(client.prepare(Command::SetSessionConfig {
                    session: session.id,
                    expected_revision: session.revision,
                    config,
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            session = updated;
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
            let output = admissions(&client, &page).await;
            assert_eq!(output.len(), 1);
            completed(&client, &page, &server, &output[0], "answer-fixture-a").await;
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
            assert!(description.contains("Choose an exact listed role"));
            assert!(description.contains("or omit it to inherit the model and instructions"));
            assert!(description.contains("the child cannot see this conversation"));
            assert!(description.contains(
                "Supply the goal, relevant paths and excerpts, constraints, edit ownership and expected result"
            ));
            assert!(description.contains("disjoint files"));
            let role = &declaration["function"]["parameters"]["properties"]["role"];
            assert!(role.get("enum").is_none());
            assert_eq!(role["type"], json!("string"));
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
                assert_eq!(tool["function"]["parameters"]["required"], json!(["task"]));
                assert_eq!(
                    tool["function"]["parameters"]["properties"]["role"]["type"],
                    json!("string")
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
            assert_eq!(child["messages"][0]["role"], "developer");
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

            assert!(prompt(child).contains("Shared default instructions"));
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
            let spawn = crate::agent_support::plugin_tool("delegation", "spawn_agent");
            let parent = Server::tools(vec![
                ("tool_search".into(), json!({"query":spawn,"limit":1})),
                (
                    spawn.clone(),
                    json!({"role": "review", "task": "Bounded child task"}),
                ),
            ])
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
            assert_eq!(turn.roles.profiles, vec![role.clone()]);
            assert_eq!(page.runs[0].status, Status::Completed);
            let output = results(&client, &page).await;
            assert_eq!(output.len(), 2, "{output:?}");
            assert_eq!(output[0]["tools"][0]["name"], spawn);
            let view = super::super::progress::observe(&client, session.id).await;
            assert_eq!(view.calls.len(), 2);
            assert_eq!(view.calls[0].name, "tool_search");
            assert_eq!(view.calls[1].name, spawn);
            let call = view
                .calls
                .iter()
                .find(|call| call.name == spawn && call.id.as_deref() == Some("call-fixture-1"))
                .expect("role call expected");
            let response = call.response.as_ref().expect("role result expected");
            let entry = page
                .entries
                .iter()
                .find(|entry| entry.id == response.entry)
                .unwrap();
            assert!(
                matches!(&entry.parts[response.index], Part::ToolResult { id, name, .. }
                if id.as_deref() == Some("call-fixture-1") && name == &spawn)
            );
            let output =
                crate::agent_support::details::tool_detail(&client, &page, entry, response.index)
                    .await
                    .result
                    .unwrap();
            if skills {
                let error: rmcp::ErrorData = serde_json::from_value(output).unwrap();
                let fault: Fault = serde_json::from_value(error.data.unwrap()).unwrap();
                assert_eq!(
                    call.failure.as_ref().and_then(|failure| failure.code),
                    Some(ErrorCode::InvalidRequest)
                );
                assert_eq!(fault.code, ErrorCode::InvalidRequest);
                assert_eq!(fault.message, "skill is not listed in this turn's catalog");
                assert_eq!(error.message, fault.message);
                assert_eq!(child.requests.lock().unwrap().len(), 0);
                assert!(children(&client, session.id, 0).await.is_empty());
            } else {
                assert!(call.failure.is_none());
                assert_eq!(output["admitted"], true);
                assert_eq!(output["status"], "queued");
                assert_eq!(child.requests.lock().unwrap().len(), 1);
                let children = children(&client, session.id, 1).await;
                let page = history(&client, children[0].id).await;
                assert_eq!(page.runs[0].status, Status::Completed);
                assert!(page.runs[0].error.is_none());
                assert_eq!(results(&client, &page).await.len(), 1);
                assert_eq!(
                    page.entries
                        .iter()
                        .rfind(|entry| entry.author == "assistant")
                        .unwrap()
                        .parts,
                    vec![Part::Text(TURN_LIMIT_MESSAGE.into())]
                );
                received(
                    &parent,
                    children[0].id,
                    &format!("completed: {TURN_LIMIT_MESSAGE}"),
                    1,
                );
            }
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
        }
    }
}
