use super::*;

fn tool(name: &str) -> String {
    crate::agent_support::plugin_tool("delegation", name)
}

async fn submit(client: &Client, session: &Session, message: &str) -> QueuedTurn {
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: message.into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn
}

async fn read(client: &Client, id: SessionId) -> Session {
    let Output::Session(session) = client
        .execute(client.prepare(Command::ReadSession { session: id }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    session
}

async fn catalog(client: &Client) -> Vec<role::Profile> {
    let Output::Roles(roles) = client
        .execute(client.prepare(Command::ListRoles))
        .await
        .unwrap()
    else {
        panic!("roles expected")
    };
    roles
}

#[tokio::test]
async fn scopes_skill_policy_to_admitted_inventory() {
    use sailry_protocol::conversation::reference::{Reference, Target};
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let unused = Server::tools(vec![]).await;
        let fixture = super::fixture::Fixture::start(remote, server, unused).await;
        for name in ["delegation", "worktrees"] {
            let Output::Plugin(info) = fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::ReadPlugin { name: name.into() }),
                )
                .await
                .unwrap()
            else {
                panic!("plugin expected")
            };
            fixture
                .client
                .execute(fixture.client.prepare(Command::SetPluginEnabled {
                    name: name.into(),
                    expected_revision: info.summary.revision,
                    enabled: true,
                }))
                .await
                .unwrap();
        }
        let mut message: Input = "Compare two independent implementations".into();
        message.references.push(Reference {
            target: Target::Skill {
                package: "delegation".into(),
                name: "compare-worktrees".into(),
            },
            label: "Compare approaches".into(),
        });
        let Output::QueuedTurn(selected) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message,
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&fixture.client, fixture.session.id, selected.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == selected.id)
                .unwrap()
                .status,
            Status::Completed
        );
        let request = fixture.parent.requests.lock().unwrap()[0].clone();
        let instruction = node_context(&request, "Work mode");
        assert_eq!(instruction.matches("<selected_skill>").count(), 1);
        assert!(instruction.contains(
            "Use this skill when the user requests two or more competing implementations"
        ));
        assert!(
            instruction.contains(
                "unless the full instructions are already supplied; do not load them again"
            )
        );

        for stage in [
            "enabled",
            "worktrees_disabled",
            "worktrees_removed",
            "delegation_disabled",
            "delegation_removed",
        ] {
            if stage != "enabled" {
                let name = if stage.starts_with("worktrees") {
                    "worktrees"
                } else {
                    "delegation"
                };
                let Output::Plugin(info) = fixture
                    .client
                    .execute(
                        fixture
                            .client
                            .prepare(Command::ReadPlugin { name: name.into() }),
                    )
                    .await
                    .unwrap()
                else {
                    panic!("plugin expected")
                };
                let command = if stage.ends_with("removed") {
                    Command::RemovePlugin {
                        name: name.into(),
                        expected_revision: info.summary.revision,
                    }
                } else {
                    Command::SetPluginEnabled {
                        name: name.into(),
                        expected_revision: info.summary.revision,
                        enabled: false,
                    }
                };
                fixture
                    .client
                    .execute(fixture.client.prepare(command))
                    .await
                    .unwrap();
            }
            let offset = fixture.parent.requests.lock().unwrap().len();
            let turn = fixture
                .push("Inspect the currently available capabilities")
                .await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                Status::Completed,
                "{stage}"
            );
            let request = fixture.parent.requests.lock().unwrap()[offset].clone();
            let tools: std::collections::BTreeSet<_> = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|tool| tool["function"]["name"].as_str())
                .collect();
            let instruction = node_context(&request, "Work mode");
            assert!(instruction.contains("1. Review this catalog and select the smallest set"));
            assert!(
                instruction.contains("When load_skill is available in this turn's tool catalog")
            );
            assert!(
                instruction.contains(
                    "do not invent capabilities or install anything without authorization"
                )
            );
            assert!(tools.contains("load_skill"));
            let delegation = !stage.starts_with("delegation");
            assert_eq!(
                instruction.contains("delegation:compare-worktrees"),
                delegation,
                "{stage}"
            );
            assert_eq!(
                turn.plugins
                    .iter()
                    .any(|package| package.name == "delegation"),
                delegation,
                "{stage}"
            );
            for name in ["spawn_agent", "guide_agent", "wait_agent"] {
                assert_eq!(
                    tools.contains(tool(name).as_str()),
                    delegation,
                    "{stage} {name}"
                );
            }
            let worktrees = stage == "enabled";
            for name in ["create_worktree", "register_worktree"] {
                assert_eq!(
                    tools.contains(plugin_tool("worktrees", name).as_str()),
                    worktrees,
                    "{stage} {name}"
                );
            }
            assert_eq!(
                turn.plugins
                    .iter()
                    .any(|package| package.name == "worktrees"),
                worktrees,
                "{stage}"
            );
            assert!(!instruction.contains("<selected_skill>"));
            assert!(results(&fixture.client, &page).await.is_empty());
        }
        assert!(
            selected
                .plugins
                .iter()
                .any(|package| package.name == "delegation")
        );
        assert!(fixture.child.requests.lock().unwrap().is_empty());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn edits_catalog_and_applies_future_capture() {
    for remote in [false, true] {
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
        let server=Server::scripted_tools(vec![
            (tool("add_agent"),json!({"key":"catalog-review","name":"Catalog review","instructions":"Inspect catalog changes","read_only":true,"max_turns":3})),
            (tool("list_agents"),json!({})),
        ]).await;
        install(&client).await;
        let (session, _) = configured_session(&client, &server.endpoint, &root).await;
        let mut config = session.config.clone();
        config.permission = Permission::Full;
        config
            .options
            .entry("delegation".into())
            .or_default()
            .insert("mode".into(), json!("multitask"));
        let Output::Session(mut session) = client
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
        let original = session.roles.clone();
        let turn = submit(&client, &session, "Add a catalog definition").await;
        let page = finished(&client, session.id, turn.id).await;
        let output = results(&client, &page).await;
        assert_eq!(output.len(), 2, "{output:?}");
        assert_eq!(output[0]["revision"], "1", "{output:?}");
        assert_eq!(
            output[1]["agents"].as_array().unwrap().len(),
            catalog(&client).await.len()
        );
        let added = catalog(&client)
            .await
            .into_iter()
            .find(|role| role.key == "catalog-review")
            .unwrap();
        assert_eq!(read(&client, session.id).await.roles, original);
        assert_eq!(turn.roles, original);
        server.replace_tools(vec![
            (
                tool("edit_agent"),
                json!({"id":added.id,"expected_revision":"1","name":"Edited catalog review"}),
            ),
            (
                tool("edit_agent"),
                json!({"id":added.id,"expected_revision":"1","instructions":"Must conflict"}),
            ),
        ]);
        let turn = submit(&client, &session, "Edit the catalog definition").await;
        let page = finished(&client, session.id, turn.id).await;
        let output = results(&client, &page).await;
        assert_eq!(output[output.len() - 2]["revision"], "2");
        assert_eq!(output.last().unwrap()["error"]["code"], "revision_conflict");
        let edited = catalog(&client)
            .await
            .into_iter()
            .find(|role| role.id == added.id)
            .unwrap();
        assert_eq!(edited.instructions, added.instructions);
        assert_eq!(edited.max_turns, added.max_turns);
        assert_eq!(edited.read_only, added.read_only);
        assert_eq!(read(&client, session.id).await.roles, original);

        let mut config = session.config.clone();
        config.permission = Permission::Ask;
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
        server.replace_tools(vec![(tool("apply_agents"),json!({"agents":[{"id":edited.id,"revision":edited.revision.to_string()}],"expected_session_revision":session.revision.to_string()}))]);
        let applying = submit(&client, &session, "Apply this exact selection").await;
        let (_, approval) = super::super::approvals::pending(&client, session.id).await;
        let Output::QueuedTurn(queued) = client
            .execute(client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Already queued capture".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        assert_eq!(queued.roles, original);
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: approval.id,
                decision: sailry_protocol::conversation::Decision::Approve,
            }))
            .await
            .unwrap();
        let page = finished(&client, session.id, applying.id).await;
        let output = results(&client, &page).await;
        let applied = output.last().unwrap();
        assert_eq!(applied["session"], session.id.to_string());
        assert_eq!(applied["revision"], (session.revision + 1).to_string());
        assert_eq!(
            applied["roles"],
            json!([{"id":edited.id,"revision":edited.revision.to_string()}])
        );
        assert!(applied.get("config").is_none());
        assert!(applied.get("providers").is_none());
        assert_eq!(applying.roles, original);
        session = read(&client, session.id).await;
        assert_eq!(session.roles.profiles, vec![edited.clone()]);

        server.replace_tools(vec![(
            tool("spawn_agent"),
            json!({"role":edited.key,"task":"Must retain the queued roster"}),
        )]);
        client
            .execute(client.prepare(Command::StartQueuedTurn { turn: queued.id }))
            .await
            .unwrap();
        let page = finished(&client, session.id, queued.id).await;
        assert_eq!(
            results(&client, &page).await.last().unwrap()["error"]["code"],
            "invalid_request"
        );
        assert_eq!(queued.roles, original);
        server.replace_tools(vec![]);
        let fresh = submit(&client, &session, "Inspect the applied roster").await;
        assert_eq!(fresh.roles.profiles, vec![edited.clone()]);
        finished(&client, session.id, fresh.id).await;
        {
            let requests = server.requests.lock().unwrap();
            let declaration = requests.last().unwrap()["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["function"]["name"] == tool("spawn_agent"))
                .unwrap();
            let role = &declaration["function"]["parameters"]["properties"]["role"];
            assert!(role.get("enum").is_none());
            assert_eq!(role["type"], json!(["string", "null"]));
            let first = requests
                .iter()
                .find_map(|request| {
                    request["tools"]
                        .as_array()?
                        .iter()
                        .find(|entry| entry["function"]["name"] == tool("spawn_agent"))
                })
                .unwrap();
            assert_eq!(
                declaration["function"]["parameters"],
                first["function"]["parameters"]
            );
            assert!(
                node_context(requests.last().unwrap(), "Work mode")
                    .contains(&format!("{} ({})", edited.key, edited.name))
            );
        }

        server.replace_tools(vec![(
            tool("delete_agent"),
            json!({"id":edited.id,"expected_revision":edited.revision.to_string()}),
        )]);
        let deleting = submit(&client, &session, "Delete only this catalog definition").await;
        let (_, approval) = super::super::approvals::pending(&client, session.id).await;
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: approval.id,
                decision: sailry_protocol::conversation::Decision::Approve,
            }))
            .await
            .unwrap();
        finished(&client, session.id, deleting.id).await;
        assert!(
            !catalog(&client)
                .await
                .iter()
                .any(|role| role.id == edited.id)
        );
        assert_eq!(
            read(&client, session.id).await.roles.profiles,
            vec![edited.clone()]
        );
        assert_eq!(deleting.roles.profiles, vec![edited.clone()]);
        let Output::Session(new) = client
            .execute(client.prepare(Command::CreateSession {
                project: session.project,
                worktree: Some(session.worktree),
                config: Some(session.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("new session expected")
        };
        assert!(!new.roles.profiles.iter().any(|role| role.id == edited.id));
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
