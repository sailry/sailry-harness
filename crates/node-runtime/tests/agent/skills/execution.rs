use super::*;

#[cfg(unix)]
#[tokio::test]
async fn runs_from_skill_and_project_directories() {
    for remote in [false, true] {
        let idle = Server::start(false).await;
        let (directory, node, controller, client, session) = fixture(remote, &idle).await;
        let Output::Plugin(installed) = execute(
            &client,
            Command::ReadPlugin {
                name: "example".into(),
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        let skill_directory = node
            .profile()
            .join("plugins/packages")
            .join(&installed.summary.digest)
            .join("skills/analysis");
        let script = skill_directory.join("scripts/query.sh");
        let server = Server::tools(vec![
            ("load_skill".into(), json!({"skill":"example:analysis"})),
            (plugin_tool("commands", "run_command"), json!({"command":"sh scripts/query.sh skill", "cwd":skill_directory})),
            (plugin_tool("commands", "run_command"), json!({"command":format!("sh '{}' project",script.to_str().unwrap().replace('\'',"'\\''")), "cwd":"."})),
        ]).await;
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let mut provider = snapshot.providers[0].clone();
        provider.endpoint = server.endpoint.clone();
        execute(
            &client,
            Command::PutProvider {
                provider,
                expected_revision: 2,
            },
        )
        .await;
        let Output::QueuedTurn(turn) = execute(
            &client,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Run the bundled script from both directories".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        for _ in 0..2 {
            let (_, approval) = approvals::pending(&client, session.id).await;
            assert_eq!(approval.source, ApprovalSource::User);
            execute(
                &client,
                Command::ResolveApproval {
                    session: session.id,
                    approval: approval.id,
                    decision: Decision::Approve,
                },
            )
            .await;
        }
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let loaded = results(&page);
        assert_eq!(loaded[0]["directory"], skill_directory.to_str().unwrap());
        let commands = process::results(&page);
        assert_eq!(commands.len(), 2);
        assert!(
            commands
                .iter()
                .all(|command| command.outcome == sailry_protocol::process::Outcome::Exited(0))
        );
        assert_eq!(
            commands[0].stdout.text,
            format!(
                "skill\n{}\n",
                skill_directory.canonicalize().unwrap().display()
            )
        );
        assert_eq!(
            commands[1].stdout.text,
            format!(
                "project\n{}\n",
                directory
                    .path()
                    .join("project")
                    .canonicalize()
                    .unwrap()
                    .display()
            )
        );
        {
            let requests = server.requests.lock().unwrap();
            assert!(
                requests[0]["messages"]
                    .to_string()
                    .contains("Resolve relative script paths against the returned skill directory")
            );
        }
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn scripts_require_approval() {
    for remote in [false, true] {
        for allow in [false, true] {
            let idle = Server::start(false).await;
            let (directory, node, controller, client, session) = fixture(remote, &idle).await;
            let Output::Plugin(installed) = execute(
                &client,
                Command::ReadPlugin {
                    name: "example".into(),
                },
            )
            .await
            else {
                panic!("plugin expected")
            };
            let script = node
                .profile()
                .join("plugins/packages")
                .join(&installed.summary.digest)
                .join("skills/analysis/check.sh");
            let command = format!("sh '{}'", script.to_str().unwrap().replace('\'', "'\\''"));
            let server = Server::tools(vec![
                ("load_skill".into(), json!({"skill": "example:analysis"})),
                (
                    plugin_tool("commands", "run_command"),
                    json!({"command": command}),
                ),
            ])
            .await;
            let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
                panic!("snapshot expected")
            };
            let mut provider = snapshot.providers[0].clone();
            provider.endpoint = server.endpoint.clone();
            execute(
                &client,
                Command::PutProvider {
                    provider,
                    expected_revision: 2,
                },
            )
            .await;
            let request = client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Load the skill and run its script".into(),
            });
            let Output::QueuedTurn(turn) = client.execute(request.clone()).await.unwrap() else {
                panic!("turn expected")
            };
            let (_, approval) = approvals::pending(&client, session.id).await;
            assert_eq!(approval.source, ApprovalSource::User);
            let count = directory.path().join("project/script-count.txt");
            assert!(!count.exists());
            assert_eq!(server.requests.lock().unwrap().len(), 2);
            execute(
                &client,
                Command::ResolveApproval {
                    session: session.id,
                    approval: approval.id,
                    decision: if allow {
                        Decision::Approve
                    } else {
                        Decision::Deny
                    },
                },
            )
            .await;
            let page = finished(&client, session.id, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
            assert_eq!(count.exists(), allow);
            if allow {
                assert_eq!(fs::read_to_string(count).unwrap(), "x");
                let output = process::results(&page);
                assert_eq!(output.len(), 1);
                assert!(output[0].stdout.text.contains("script output 中文 🙂"));
            }
            assert_eq!(
                client.execute(request).await.unwrap(),
                Output::QueuedTurn(turn)
            );
            assert_eq!(server.requests.lock().unwrap().len(), 3);
            drop(client);
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn loads_complete_resources() {
    for remote in [false, true] {
        let calls = vec![
            ("load_skill".into(), json!({"skill": "example:analysis"})),
            (
                "read_skill_resource".into(),
                json!({"skill": "example:analysis", "path": "references/guide.md"}),
            ),
            (
                "read_skill_resource".into(),
                json!({"skill": "example:analysis", "path": "../analysis/SKILL.md"}),
            ),
            ("load_skill".into(), json!({"skill": "missing:analysis"})),
        ];
        let server = Server::tools(calls.clone()).await;
        let (directory, node, controller, client, session) = fixture(remote, &server).await;
        let Output::Plugins(packages) = execute(&client, Command::ListPlugins).await else {
            panic!("plugins expected")
        };
        assert!(
            packages
                .iter()
                .all(|package| package.name != "sailry.builtin.skills")
        );
        for package in packages {
            if package.name != "example" && package.enabled {
                execute(
                    &client,
                    Command::SetPluginEnabled {
                        name: package.name,
                        expected_revision: package.revision,
                        enabled: false,
                    },
                )
                .await;
            }
        }
        let body = fs::read_to_string(
            directory
                .path()
                .join("project/package/skills/analysis/SKILL.md"),
        )
        .unwrap();
        let request = client.prepare(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Use the analysis skill".into(),
        });
        let Output::QueuedTurn(turn) = client.execute(request.clone()).await.unwrap() else {
            panic!("turn expected")
        };
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        super::super::evaluation::trajectory(&page, &calls);
        let output = results(&page);
        assert_eq!(output.len(), 4);
        assert_eq!(output[0]["content"], body);
        assert_eq!(output[1]["content"], "Guide 1.0.0 中文 🙂");
        assert!(output[2].get("error").is_some());
        assert!(output[3].get("error").is_some());
        assert!(page.approvals.is_empty());
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 5);
        let initial = requests[0]["messages"].to_string();
        assert!(initial.contains("example:analysis"));
        assert!(!initial.contains("Complete end"));
        let tool = requests.last().unwrap()["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "tool")
            .unwrap();
        let result: Value = serde_json::from_str(tool["content"].as_str().unwrap()).unwrap();
        assert_eq!(result["content"], body);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&client, session.id).await, page);
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        assert_eq!(server.requests.lock().unwrap().len(), 5);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn injects_frozen_roles() {
    for remote in [false, true] {
        let server = Server::parallel(vec![(
            crate::agent_support::plugin_tool("delegation", "spawn_agent"),
            json!({"role": "review", "task": "Use the selected skill"}),
        )])
        .await;
        let (directory, node, controller, client, session) = fixture(remote, &server).await;
        let role = role::Profile {
            appearance: None,
            id: RoleId::new(),
            revision: 0,
            key: "review".into(),
            name: "Review".into(),
            description: "Review data".into(),
            model: None,
            max_turns: Some(2),
            skills: vec!["example:analysis".into()],
            instructions: "Keep {literal_role_state} unchanged".into(),
        };
        let Output::Role(role) = execute(
            &client,
            Command::PutRole {
                role,
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("role expected")
        };
        let Output::Session(session) = execute(
            &client,
            Command::SetSessionRoles {
                session: session.id,
                expected_revision: session.revision,
                roles: vec![role.reference()],
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(turn) = execute(
            &client,
            Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Delegate review".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        let body = fs::read_to_string(
            directory
                .path()
                .join("project/package/skills/analysis/SKILL.md"),
        )
        .unwrap();
        package(&directory.path().join("project"), "2.0.0");
        install(&client, session.worktree, 1).await;
        execute(
            &client,
            Command::RemovePlugin {
                name: "example".into(),
                expected_revision: 2,
            },
        )
        .await;
        execute(&client, Command::StartQueuedTurn { turn: turn.id }).await;
        let page = finished(&client, session.id, turn.id).await;
        let output = results(&page);
        assert_eq!(output[0]["status"], "completed", "{output:?}");
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        let child = requests
            .iter()
            .find(|request| {
                request["tools"].as_array().unwrap().iter().all(|tool| {
                    tool["function"]["name"]
                        != crate::agent_support::plugin_tool("delegation", "spawn_agent")
                })
            })
            .unwrap();
        let instructions = child["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|message| message["content"].as_str())
            .collect::<String>();
        assert!(instructions.contains(&body));
        assert!(instructions.contains("Keep {literal_role_state} unchanged"));
        assert!(!instructions.contains("Version 2.0.0"));
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let child = snapshot
            .sessions
            .iter()
            .find(|session| session.delegation.is_some())
            .unwrap();
        let parent_turn = snapshot
            .turns
            .iter()
            .find(|turn| turn.session == session.id)
            .unwrap();
        let child_turn = snapshot
            .turns
            .iter()
            .find(|turn| turn.session == child.id)
            .unwrap();
        assert_eq!(child_turn.plugins, parent_turn.plugins);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
