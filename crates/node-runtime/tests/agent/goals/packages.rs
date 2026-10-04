use super::*;

async fn execute(fixture: &Fixture, command: Command) -> Output {
    fixture
        .client
        .execute(fixture.client.prepare(command))
        .await
        .unwrap()
}

fn source(fixture: &Fixture, name: &str, suffix: &str) {
    let root = fixture.root.join(name);
    std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
    let mut manifest: Value =
        serde_json::from_str(include_str!("../../../../../plugins/goals/plugin.json")).unwrap();
    manifest["name"] = json!(name);
    let extension = &mut manifest["extensions"]["dev.sailry.platform"];
    extension.as_object_mut().unwrap().remove("desktop");
    extension.as_object_mut().unwrap().remove("ui");
    extension["host"]["commands"][0]["name"] = json!(name);
    for tool in extension["tools"].as_array_mut().unwrap() {
        tool["display"]["label"] = json!(format!("Record{suffix}"));
    }
    let code = include_str!("../../../../../plugins/goals/dev.sailry.platform/host/main.js")
        .replace(
            "description:description(args.description)",
            &format!("description:description(args.description) + {suffix:?}"),
        );
    std::fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
    std::fs::write(root.join("dev.sailry.platform/host/main.js"), code).unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/host/labels.js"),
        include_str!("../../../../../plugins/goals/dev.sailry.platform/host/labels.js"),
    )
    .unwrap();
}

async fn install(fixture: &Fixture, name: &str, revision: u64) -> plugin::Info {
    let Output::Plugin(info) = execute(
        fixture,
        Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: name.into(),
            name: name.into(),
            expected_revision: revision,
        },
    )
    .await
    else {
        panic!("package expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

#[tokio::test]
async fn retains_captured_code_and_history_after_removal() {
    for remote in [false, true] {
        let name = "outcomes";
        let server = Server::tools(vec![(
            plugin_tool(name, "create_goal"),
            json!({"description":"Evidence"}),
        )])
        .await;
        let fixture = Fixture::new(remote, &server).await;
        source(&fixture, name, " original");
        let original = install(&fixture, name, 0).await;
        let Output::Plugins(packages) = execute(&fixture, Command::ListPlugins).await else {
            panic!("plugins expected")
        };
        let others: Vec<_> = packages
            .iter()
            .filter(|package| package.enabled && package.name != name)
            .map(|package| package.name.as_str())
            .collect();
        disable_tools(&fixture.client, &others).await;
        let Output::QueuedTurn(queued) = execute(
            &fixture,
            Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: 1,
                message: "Record the requested goal".into(),
            },
        )
        .await
        else {
            panic!("queued turn expected")
        };
        assert_eq!(queued.plugins, [original.summary.reference()]);
        source(&fixture, name, " updated");
        install(&fixture, name, 1).await;
        execute(&fixture, Command::StartQueuedTurn { turn: queued.id }).await;
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let saved = results(&page)
            .iter()
            .find(|(tool, _)| *tool == plugin_tool(name, "create_goal"))
            .unwrap()
            .1
            .clone();
        assert_eq!(saved["goal"]["description"], "Evidence original");
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part,Part::ToolCall {display:Some(display),..} if display.label == "Record original")));
        execute(
            &fixture,
            Command::RemovePlugin {
                name: name.into(),
                expected_revision: 2,
            },
        )
        .await;
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start(profile).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&client, fixture.session.id).await, page);
        let Output::Plugins(packages) = client
            .execute(client.prepare(Command::ListPlugins))
            .await
            .unwrap()
        else {
            panic!("plugins expected")
        };
        assert!(!packages.iter().any(|package| package.name == name));
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn commands_and_new_turns_follow_enablement() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        source(&fixture, "outcomes", "");
        install(&fixture, "outcomes", 0).await;
        let command = || {
            fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: 1,
                message: "/outcomes".into(),
            })
        };
        let Output::PluginResult(value) = fixture.client.execute(command()).await.unwrap() else {
            panic!("registered command expected")
        };
        assert_eq!(value["ui_intent"], "goal");
        for (revision, enabled) in [(1, false), (2, true)] {
            execute(
                &fixture,
                Command::SetPluginEnabled {
                    name: "outcomes".into(),
                    expected_revision: revision,
                    enabled,
                },
            )
            .await;
            let turn = approvals::submit(&fixture.client, fixture.session.id).await;
            finished(&fixture.client, fixture.session.id, turn).await;
            let requests = server.requests.lock().unwrap();
            let tools = requests.last().unwrap()["tools"].as_array().unwrap();
            for name in ["create_goal", "get_goal", "update_goal"] {
                assert_eq!(
                    tools
                        .iter()
                        .any(|tool| tool["function"]["name"] == plugin_tool("outcomes", name)),
                    enabled
                );
            }
        }
        execute(
            &fixture,
            Command::RemovePlugin {
                name: "outcomes".into(),
                expected_revision: 3,
            },
        )
        .await;
        let Output::QueuedTurn(turn) = fixture.client.execute(command()).await.unwrap() else {
            panic!("unregistered slash input must remain ordinary text")
        };
        finished(&fixture.client, fixture.session.id, turn.id).await;
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
