use super::*;

#[tokio::test]
async fn standalone_credentials() {
    use plugin::{mcp::Definition, settings::SecretUpdate};
    use std::collections::BTreeMap;

    for remote in [false, true] {
        let name = "mcp-reporting";
        let model = Server::turn_tools(vec![(
            peer::package_alias(name, "server", "read"),
            json!({}),
        )])
        .await;
        let fixture = process::Fixture::new(remote, &model).await;
        let Output::Plugins(initial) = execute(&fixture.client, Command::ListPlugins).await else {
            panic!("plugin list expected")
        };
        for package in initial.into_iter().filter(|package| package.enabled) {
            execute(
                &fixture.client,
                Command::SetPluginEnabled {
                    name: package.name,
                    expected_revision: package.revision,
                    enabled: false,
                },
            )
            .await;
        }
        let mut config = peer::config("mcp::peer::stdio_peer", "settings");
        let original = "original-standalone-${PLUGIN_ROOT}-credential";
        config["env"]["SAILRY_MCP_TOKEN"] = json!(original);
        let definition = Definition::Stdio {
            command: config["command"].as_str().unwrap().into(),
            args: serde_json::from_value(config["args"].clone()).unwrap(),
            env: config["env"].as_object().unwrap().keys().cloned().collect(),
        };
        let mut secrets: BTreeMap<_, _> = config["env"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    SecretUpdate::Replace(Secret::new(value.as_str().unwrap().into())),
                )
            })
            .collect();
        let Output::Plugin(first) = execute(
            &fixture.client,
            Command::InstallMcp {
                name: name.into(),
                expected_revision: 0,
                definition: definition.clone(),
                secrets: secrets.clone(),
            },
        )
        .await
        else {
            panic!("MCP package expected")
        };

        let (_, queued) = submit(&fixture, true).await;
        assert_eq!(queued.plugins, [first.summary.reference()]);
        assert_eq!(queued.revision, fixture.session.revision);
        for value in secrets.values_mut() {
            *value = SecretUpdate::Keep;
        }
        let replacement = "replacement-standalone-credential";
        secrets.insert(
            "SAILRY_MCP_TOKEN".into(),
            SecretUpdate::Replace(Secret::new(replacement.into())),
        );
        let Output::Plugin(updated) = execute(
            &fixture.client,
            Command::InstallMcp {
                name: name.into(),
                expected_revision: first.summary.revision,
                definition,
                secrets,
            },
        )
        .await
        else {
            panic!("MCP package expected")
        };
        assert_ne!(
            updated.summary.settings_revision,
            first.summary.settings_revision
        );
        execute(
            &fixture.client,
            Command::StartQueuedTurn { turn: queued.id },
        )
        .await;
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let output = results(&page);
        assert_eq!(
            output[0]["output"]["secret_hash"],
            blake3::hash(original.as_bytes()).to_hex().to_string()
        );
        assert_eq!(output[0]["output"]["settings"], json!({}));
        let public = serde_json::to_string(&page).unwrap();
        assert!(!public.contains(original) && !public.contains(replacement));
        let (_, turn) = submit(&fixture, false).await;
        assert_eq!(turn.plugins, [updated.summary.reference()]);
        assert_eq!(turn.revision, queued.revision);
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(
            results(&page).last().unwrap()["output"]["secret_hash"],
            blake3::hash(replacement.as_bytes()).to_hex().to_string()
        );
        fixture.node.shutdown().await.unwrap();
        reaped(
            &fixture
                .directory
                .path()
                .join("node/plugins/data")
                .join(name),
        );
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn recovers_frozen_approval() {
    for remote in [false, true] {
        let content = "完整 MCP 内容🙂".repeat(1500);
        let model = Server::tools(vec![
            (alias("native", "read"), json!({})),
            (alias("native", "write"), json!({"value": content})),
            (alias("native", "write"), json!({"value": "denied"})),
        ])
        .await;
        let fixture = process::Fixture::new(remote, &model).await;
        let config = json!({"native": peer::config("mcp::peer::stdio_peer", "normal"), "invalid": {"type": "unknown"}, "missing": {"type": "stdio", "command": "sailry_missing_mcp_fixture_executable"}});
        package(&fixture.root, config.clone(), "old version 中文 🙂");
        let manifest = fixture.root.join("package/plugin.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1","actions":[],"tools":[{"server":"native","name":"read","presentation":"summary"},{"server":"native","name":"write","presentation":"details"}]}});
        fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        let installed = install(&fixture, 0).await;
        assert_eq!(installed.mcp.len(), 2);
        assert_eq!(installed.skills.len(), 1);

        let (request, turn) = submit(&fixture, true).await;
        let mut permission = fixture.session.config.clone();
        permission.permission = sailry_protocol::Permission::Full;
        execute(
            &fixture.client,
            Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config: permission,
            },
        )
        .await;
        package(&fixture.root, config, "new version");
        install(&fixture, 1).await;
        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: 2,
                enabled: false,
            },
        )
        .await;
        execute(&fixture.client, Command::StartQueuedTurn { turn: turn.id }).await;
        let data = fixture.node.profile().join("plugins/data/example");
        let (page, first) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert_eq!(first.source, ApprovalSource::User);
        let call = page
            .entries
            .iter()
            .find(|entry| entry.id == first.entry)
            .unwrap();
        let Part::ToolCall { arguments, .. } = &call.parts[first.index] else {
            panic!("approval call expected")
        };
        assert_eq!(arguments["value"], content);
        for part in page.entries.iter().flat_map(|entry| &entry.parts) {
            if let Part::ToolCall {
                name, presentation, ..
            } = part
            {
                assert_eq!(
                    *presentation,
                    if name == &alias("native", "read") {
                        sailry_protocol::tool::Presentation::Summary
                    } else {
                        sailry_protocol::tool::Presentation::Details
                    }
                );
            }
        }
        let output = results(&page);
        assert_eq!(output[0]["output"]["version"], "old version 中文 🙂");
        assert_eq!(
            output[0]["output"]["root"],
            fixture
                .node
                .profile()
                .join("plugins/packages")
                .join(&installed.summary.digest)
                .to_str()
                .unwrap()
        );
        assert_eq!(output[0]["output"]["data"], data.to_str().unwrap());
        assert_eq!(output[0]["output"]["cwd"], data.to_str().unwrap());
        assert!(
            output[0]["output"]["text"]
                .as_str()
                .unwrap()
                .ends_with("|${HOME}|中文 🙂")
        );
        assert_eq!(output[0]["output"]["provider_key_present"], false);
        assert!(!data.join("effects").exists());
        fixture
            .controller
            .handle()
            .disconnect(fixture.node.id())
            .await;
        let (decision, receipt) = process::decide(&fixture.client, &first, Decision::Approve).await;
        let (_, second) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert_ne!(first.id, second.id);
        process::decide(&fixture.client, &second, Decision::Deny).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert_eq!(fs::read_to_string(data.join("effects")).unwrap(), content);
        assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "x");
        assert_eq!(
            fixture.client.execute(decision.clone()).await.unwrap(),
            receipt
        );
        assert_eq!(
            fixture.client.execute(request.clone()).await.unwrap(),
            Output::QueuedTurn(turn.clone())
        );
        let process::Fixture {
            directory,
            node,
            controller,
            client,
            session,
            ..
        } = fixture;
        drop(client);
        node.shutdown().await.unwrap();
        reaped(&data);
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&client, session.id).await, page);
        assert_eq!(client.execute(decision).await.unwrap(), receipt);
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        assert_eq!(fs::read_to_string(data.join("effects")).unwrap(), content);
        assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "x");
        assert_eq!(model.requests.lock().unwrap().len(), 4);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn lost_results_prevent_replay() {
    for remote in [false, true] {
        let model = Server::tools(vec![(alias("native", "fail"), json!({"value": "once"}))]).await;
        let mut fixture = process::Fixture::new(remote, &model).await;
        package(
            &fixture.root,
            json!({"native": peer::config("mcp::peer::stdio_peer", "normal")}),
            "version",
        );
        install(&fixture, 0).await;

        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Full;
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
        let (request, turn) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert_eq!(page.approvals[0].source, ApprovalSource::Full);
        let result = results(&page);
        assert_eq!(result[0]["error"]["code"], "outcome_unknown");
        let data = fixture.node.profile().join("plugins/data/example");
        assert_eq!(fs::read_to_string(data.join("effects")).unwrap(), "once");
        reaped(&data);
        fixture.client.execute(request).await.unwrap();
        assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "x");
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn closes_children() {
    for remote in [false, true] {
        for mode in ["normal", "slow_handshake"] {
            for shutdown in [false, true] {
                let model = Server::tools(vec![(alias("native", "hold"), json!({}))]).await;
                let fixture = process::Fixture::new(remote, &model).await;
                package(
                    &fixture.root,
                    json!({"native": peer::config("mcp::peer::stdio_peer", mode)}),
                    "version",
                );
                install(&fixture, 0).await;

                let (_, turn) = submit(&fixture, false).await;
                let data = fixture.node.profile().join("plugins/data/example");
                present(&data.join("pid")).await;
                if mode == "normal" {
                    let (_, approval) =
                        approvals::pending(&fixture.client, fixture.session.id).await;
                    process::decide(&fixture.client, &approval, Decision::Approve).await;
                    present(&data.join("holding")).await;
                }
                if shutdown {
                    fixture.node.shutdown().await.unwrap();
                } else {
                    execute(&fixture.client, Command::StopTurn { turn: turn.id }).await;
                    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
                    assert_eq!(page.runs[0].status, Status::Cancelled, "{:?}", page.runs);
                    reaped(&data);
                    fixture.node.shutdown().await.unwrap();
                }
                reaped(&data);
                fixture.controller.close().await.unwrap();
            }
        }
    }
}
