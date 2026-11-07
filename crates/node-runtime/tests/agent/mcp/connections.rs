use super::*;

fn pid(data: &Path) -> i32 {
    fs::read_to_string(data.join("pid"))
        .unwrap()
        .parse()
        .unwrap()
}

pub(super) fn requests(data: &Path, method: &str) -> usize {
    fs::read_to_string(data.join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|request| request["method"] == method)
        .count()
}

#[tokio::test]
async fn binds_reused_inputs() {
    use question::{Answer, Response};
    use std::sync::atomic::Ordering;
    for sse in [false, true] {
        for task in [false, true] {
            let peer = if task {
                input::Server::task(
                    json!({"mode":"url","url":"https://example.invalid/continue"}),
                    sse,
                )
                .await
            } else {
                input::Server::url("https://example.invalid/continue", sse).await
            };
            let model = Server::turn_tools(vec![(alias("input", "read"), json!({}))]).await;
            let fixture = process::Fixture::packages(&model, &[]).await;
            peer.package(&fixture.root.join("package"));
            install(&fixture, 0).await;

            for response in [Response::Answer(Answer::Opened), Response::Decline] {
                let (_, turn) = submit(&fixture, false).await;
                let (_, question) = questions::pending(&fixture.client, fixture.session.id).await;
                assert_eq!(question.turn, turn.id);
                execute(
                    &fixture.client,
                    Command::ResolveQuestion {
                        session: fixture.session.id,
                        question: question.id,
                        response,
                    },
                )
                .await;
                let page = finished(&fixture.client, fixture.session.id, turn.id).await;
                assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                fixture
                    .controller
                    .handle()
                    .disconnect(fixture.node.id())
                    .await;
            }
            assert_eq!(peer.initializations.load(Ordering::SeqCst), 1);
            assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
            assert_eq!(model.requests.lock().unwrap().len(), 4);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn replaces_changed_connections() {
    let model = Server::turn_tools(vec![(alias("native", "read"), json!({}))]).await;
    let fixture = process::Fixture::packages(&model, &[]).await;
    let servers = json!({"native":peer::config("mcp::peer::stdio_peer", "normal")});
    package(&fixture.root, servers.clone(), "first");
    install(&fixture, 0).await;

    let data = fixture.node.profile().join("plugins/data/example");
    let mut original = None;
    for _ in 0..2 {
        let (_, turn) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(
            results(&fixture.client, &page).await.last().unwrap()["structuredContent"]["version"],
            "first"
        );
        assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "x");
        assert_eq!(*original.get_or_insert(pid(&data)), pid(&data));
        fixture
            .controller
            .handle()
            .disconnect(fixture.node.id())
            .await;
    }
    package(&fixture.root, servers, "second");
    install(&fixture, 1).await;

    let (_, turn) = submit(&fixture, false).await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(
        results(&fixture.client, &page).await.last().unwrap()["structuredContent"]["version"],
        "second"
    );
    assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "xx");
    // SAFETY: signal 0 only probes the recorded fixture PID and passes no pointers.
    assert_ne!(unsafe { libc::kill(original.unwrap(), 0) }, 0);
    fixture.node.shutdown().await.unwrap();
    reaped(&data);
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn reuses_tool_lists_until_changed() {
    // Notifications and an explicit zero TTL both require fresh pages on the next bind.
    for (mode, listed) in [("normal", 2), ("list_changed", 4), ("zero_ttl", 6)] {
        let model = Server::turn_tools(vec![(alias("native", "read"), json!({}))]).await;
        let fixture = process::Fixture::packages(&model, &[]).await;
        package(
            &fixture.root,
            json!({"native":peer::config("mcp::peer::stdio_peer", mode)}),
            "pooled",
        );
        install(&fixture, 0).await;

        let data = fixture.node.profile().join("plugins/data/example");
        super::sharing::warmed(&data, 1).await;
        for round in 1..=2 {
            let (_, turn) = submit(&fixture, false).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{mode}"
            );
            assert_eq!(
                results(&fixture.client, &page).await.last().unwrap()["structuredContent"]["version"],
                "pooled",
                "{mode}"
            );
            assert_eq!(requests(&data, "initialize"), 1, "{mode}");
            assert_eq!(requests(&data, "tools/call"), round, "{mode}");
            assert_eq!(
                requests(&data, "tools/list"),
                if round == 1 {
                    if mode == "zero_ttl" { 4 } else { 2 }
                } else {
                    listed
                },
                "{mode}"
            );
        }
        assert_eq!(
            fs::read_to_string(data.join("launches")).unwrap(),
            "x",
            "{mode}"
        );
        fixture.node.shutdown().await.unwrap();
        reaped(&data);
        fixture.controller.close().await.unwrap();
    }
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn replaces_changed_credentials() {
    use crate::connector_fixture as connectors;
    use plugin::connector::{Input, Kind, State};
    use std::collections::BTreeMap;

    let model = Server::turn_tools(vec![(alias("native", "read"), json!({}))]).await;
    let fixture = process::Fixture::packages(&model, &[]).await;
    isolate(&fixture.client).await;
    let mut server = peer::config("mcp::peer::stdio_peer", "credentials");
    server["env"]["SAILRY_MCP_TOKEN"] = json!("");
    server["env"]["SAILRY_MCP_SECOND"] = json!("");
    package(&fixture.root, json!({"native":server}), "0.1.0");
    connectors::configure(
        &fixture.root.join("package"),
        json!({
            "id":"service","display":{"label":"Service"},"accounts":"single",
            "origins":[connectors::ORIGIN],"auth":[{"kind":"secret","fields":["token"]}],
            "fields":[{"name":"token","title":"Token","secret":true}],
            "inject":[{"server":"native","env":"SAILRY_MCP_TOKEN","field":"token"},
                {"server":"native","env":"SAILRY_MCP_SECOND","field":"token"}]
        }),
    );
    let installed = install(&fixture, 0).await;
    assert!(installed.issues.is_empty(), "{:?}", installed.issues);
    let package = installed.summary.reference();
    let first = "first-private-credential";
    let second = "second-private-credential";
    let mut row = connectors::connected(
        &fixture.client,
        fixture.client.prepare(Command::StartConnectorLogin {
            package: package.clone(),
            connector: "service".into(),
            connection: None,
            expected_revision: None,
            method: Kind::Secret,
            input: Input {
                secrets: BTreeMap::from([("token".into(), Secret::new(first.into()))]),
                ..Default::default()
            },
        }),
    )
    .await;
    assert_eq!(row.state, State::Connected);
    let data = fixture.node.profile().join("plugins/data/example");
    super::sharing::warmed(&data, 1).await;
    assert!(model.requests.lock().unwrap().is_empty());
    assert_eq!(requests(&data, "tools/call"), 0);
    let mut original = None;
    for (index, secret) in [first, first, second].into_iter().enumerate() {
        if index == 2 {
            let Output::Connection(updated) = execute(
                &fixture.client,
                Command::UpdateConnection {
                    connection: row.id,
                    expected_revision: row.revision,
                    input: Input {
                        secrets: BTreeMap::from([("token".into(), Secret::new(second.into()))]),
                        ..Default::default()
                    },
                },
            )
            .await
            else {
                panic!("connection expected")
            };
            assert_eq!(updated.id, row.id);
            assert_eq!(updated.revision, row.revision + 1);
            assert_eq!(updated.state, State::NotConnected);
            let reconnected = connectors::connected(
                &fixture.client,
                fixture.client.prepare(Command::StartConnectorLogin {
                    package: package.clone(),
                    connector: "service".into(),
                    connection: Some(updated.id),
                    expected_revision: Some(updated.revision),
                    method: Kind::Secret,
                    input: Input::default(),
                }),
            )
            .await;
            assert_eq!(reconnected.id, row.id);
            assert_eq!(reconnected.state, State::Connected);
            row = reconnected;
            super::sharing::warmed(&data, 2).await;
            assert_eq!(requests(&data, "tools/call"), 2);
        }
        let (_, turn) = submit(&fixture, false).await;
        assert!(turn.plugins.contains(&package));
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let hash = blake3::hash(secret.as_bytes());
        assert_eq!(
            results(&fixture.client, &page).await.last().unwrap()["structuredContent"]["hashes"],
            json!([hash.as_bytes(), hash.as_bytes()])
        );
        let connections = if index == 2 { 2 } else { 1 };
        assert_eq!(requests(&data, "initialize"), connections);
        assert_eq!(requests(&data, "tools/list"), connections * 2);
        assert_eq!(requests(&data, "tools/call"), index + 1);
        if index == 2 {
            assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "xx");
            // SAFETY: signal 0 only probes the recorded fixture PID and passes no pointers.
            assert_ne!(unsafe { libc::kill(original.unwrap(), 0) }, 0);
        } else {
            assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "x");
            assert_eq!(*original.get_or_insert(pid(&data)), pid(&data));
        }
        for token in [first, second] {
            assert!(!serde_json::to_string(&page).unwrap().contains(token));
            assert!(
                !serde_json::to_string(&*model.requests.lock().unwrap())
                    .unwrap()
                    .contains(token)
            );
        }
    }
    for _ in 0..2 {
        let Output::Connection(checked) = execute(
            &fixture.client,
            Command::ProbeConnection { connection: row.id },
        )
        .await
        else {
            panic!("connection expected")
        };
        assert_eq!(checked.state, State::Connected);
        row = checked;
    }
    let Output::Connection(disabled) = execute(
        &fixture.client,
        Command::SetConnectionEnabled {
            connection: row.id,
            expected_revision: row.revision,
            enabled: false,
        },
    )
    .await
    else {
        panic!("connection expected")
    };
    execute(
        &fixture.client,
        Command::SetConnectionEnabled {
            connection: disabled.id,
            expected_revision: disabled.revision,
            enabled: true,
        },
    )
    .await;
    super::sharing::warmed(&data, 3).await;
    assert_eq!(requests(&data, "initialize"), 3);
    assert_eq!(requests(&data, "tools/list"), 6);
    assert_eq!(requests(&data, "tools/call"), 3);
    fixture.node.shutdown().await.unwrap();
    reaped(&data);
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn orders_concurrent_servers() {
    let model = Server::tools(vec![(
        "tool_search".into(),
        json!({"query":"alpha zeta","limit":8}),
    )])
    .await;
    let fixture = process::Fixture::packages(&model, &[]).await.local();
    // The failing server sits between the healthy ones in configuration order.
    package(
        &fixture.root,
        json!({
            "alpha": peer::config("mcp::peer::stdio_peer", "normal"),
            "broken": peer::config("mcp::peer::stdio_peer", "bad_handshake"),
            "zeta": peer::config("mcp::peer::stdio_peer", "normal"),
        }),
        "concurrent",
    );
    install(&fixture, 0).await;

    let (_, turn) = submit(&fixture, false).await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{:?}",
        page.runs
    );
    let requests = model.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    let loaded: Vec<_> = requests[1]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .filter(|name| name.starts_with("mcp__"))
        .map(str::to_owned)
        .collect();
    // The catalog presents tools by name, independent of connection completion.
    let mut expected: Vec<_> = ["alpha", "zeta"]
        .into_iter()
        .flat_map(|server| {
            ["read", "write", "hold", "fail"].map(|tool| peer::exposed(server, tool))
        })
        .collect();
    expected.sort();
    assert_eq!(loaded, expected);
    let system = requests[0]["messages"][0].to_string();
    let context = requests[0]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!system.contains("MCP server example/"));
    assert!(context.contains("MCP server example/broken is unavailable"));
    assert!(!context.contains("MCP server example/alpha is unavailable"));
    assert!(!context.contains("MCP server example/zeta is unavailable"));
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn keeps_idle_connections_after_stop() {
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let model = Server::held_after_tool(vec![(alias("native", "read"), json!({}))], release).await;
    let fixture = process::Fixture::packages(&model, &[]).await.local();
    package(
        &fixture.root,
        json!({"native":peer::config("mcp::peer::stdio_peer", "normal")}),
        "stopped",
    );
    install(&fixture, 0).await;

    let (_, turn) = submit(&fixture, false).await;
    // The read result is complete; only the next model response is pending.
    tokio::time::timeout(Duration::from_secs(10), async {
        while model.requests.lock().unwrap().len() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("model request deadline");
    execute(&fixture.client, Command::StopTurn { turn: turn.id }).await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(page.runs[0].status, Status::Cancelled, "{:?}", page.runs);
    let data = fixture.node.profile().join("plugins/data/example");
    // SAFETY: signal 0 only probes the recorded fixture PID and passes no pointers.
    assert_eq!(unsafe { libc::kill(pid(&data), 0) }, 0);
    fixture.node.shutdown().await.unwrap();
    reaped(&data);
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn retains_connections_after_model_failure() {
    // A rejected model request fails the turn after its servers were bound.
    let model = Server::http(usize::MAX, 400).await;
    let fixture = process::Fixture::packages(&model, &[]).await.local();
    package(
        &fixture.root,
        json!({"native":peer::config("mcp::peer::stdio_peer", "normal")}),
        "failed",
    );
    install(&fixture, 0).await;

    let data = fixture.node.profile().join("plugins/data/example");
    for _ in 0..2 {
        let (_, turn) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Failed,
            "{:?}",
            page.runs
        );
        assert_eq!(requests(&data, "initialize"), 1);
    }
    fixture.node.shutdown().await.unwrap();
    reaped(&data);
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn shares_across_sessions() {
    let model = Server::turn_tools(vec![(alias("native", "read"), json!({}))]).await;
    let mut fixture = process::Fixture::packages(&model, &[]).await;
    package(
        &fixture.root,
        json!({"native":peer::config("mcp::peer::stdio_peer", "normal")}),
        "pooled",
    );
    install(&fixture, 0).await;

    let data = fixture.node.profile().join("plugins/data/example");
    let mut children = Vec::new();
    for _ in 0..9 {
        let Output::Session(session) = execute(
            &fixture.client,
            Command::CreateSession {
                project: fixture.session.project,
                worktree: Some(fixture.session.worktree),
                config: Some(fixture.session.config.clone()),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let (_, turn) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        children.push(pid(&data));
    }
    assert!(children.iter().all(|pid| *pid == children[0]));
    assert_eq!(requests(&data, "initialize"), 1);
    assert_eq!(requests(&data, "tools/list"), 2);
    assert_eq!(requests(&data, "tools/call"), 9);
    fixture.node.shutdown().await.unwrap();
    // SAFETY: each signal-0 call checks fixture exit without sending a signal.
    assert!(
        children
            .iter()
            .all(|pid| unsafe { libc::kill(*pid, 0) } != 0)
    );
    fixture.controller.close().await.unwrap();
}
