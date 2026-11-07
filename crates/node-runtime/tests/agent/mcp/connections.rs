use super::*;

fn pid(data: &Path) -> i32 {
    fs::read_to_string(data.join("pid"))
        .unwrap()
        .parse()
        .unwrap()
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
    assert!(system.contains("MCP server example/broken is unavailable"));
    assert!(!system.contains("MCP server example/alpha is unavailable"));
    assert!(!system.contains("MCP server example/zeta is unavailable"));
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
async fn closes_failed_turn_connections() {
    // A rejected model request fails the turn after its servers were bound.
    let model = Server::http(usize::MAX, 400).await;
    let fixture = process::Fixture::packages(&model, &[]).await.local();
    package(
        &fixture.root,
        json!({"native":peer::config("mcp::peer::stdio_peer", "normal")}),
        "failed",
    );
    install(&fixture, 0).await;

    let (_, turn) = submit(&fixture, false).await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(page.runs[0].status, Status::Failed, "{:?}", page.runs);
    let data = fixture.node.profile().join("plugins/data/example");
    reaped(&data);
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn closes_idle_children() {
    let model = Server::tools(vec![(alias("native", "read"), json!({}))]).await;
    let mut fixture = process::Fixture::packages(&model, &[]).await.local();
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
    // SAFETY: signal 0 only probes this recorded fixture PID and passes no pointers.
    assert_ne!(unsafe { libc::kill(children[0], 0) }, 0);
    // SAFETY: each signal-0 call is a read-only probe of a recorded fixture PID.
    assert!(
        children[1..]
            .iter()
            .all(|pid| unsafe { libc::kill(*pid, 0) } == 0)
    );
    fixture.node.shutdown().await.unwrap();
    // SAFETY: each signal-0 call checks fixture exit without sending a signal.
    assert!(
        children
            .iter()
            .all(|pid| unsafe { libc::kill(*pid, 0) } != 0)
    );
    fixture.controller.close().await.unwrap();
}
