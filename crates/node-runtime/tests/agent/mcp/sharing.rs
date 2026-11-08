use super::{connections::requests, *};

async fn another_session(fixture: &mut process::Fixture) {
    let root = fixture.directory.path().join("sharing-project");
    fs::create_dir(&root).unwrap();
    let Output::Project(project) = execute(
        &fixture.client,
        Command::RegisterProject {
            name: "Sharing project".into(),
            path: root.to_str().unwrap().into(),
        },
    )
    .await
    else {
        panic!("project expected");
    };
    let Output::Session(session) = execute(
        &fixture.client,
        Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(fixture.session.config.clone()),
        },
    )
    .await
    else {
        panic!("session expected");
    };
    fixture.session = session;
}

#[tokio::test]
async fn isolates_rpc_errors_between_sessions() {
    let model = Server::turn_tools(vec![
        (alias("native", "read"), json!({"value":"fail"})),
        (alias("native", "read"), json!({"value":"complete"})),
    ])
    .await;
    let mut fixture = process::Fixture::packages(&model, &[]).await;
    package(
        &fixture.root,
        json!({"native":peer::config("mcp::peer::stdio_peer","rpc_error")}),
        "shared",
    );
    install(&fixture, 0).await;
    let first = fixture.session.id;
    let (_, first_turn) = submit(&fixture, false).await;
    let data = fixture.node.profile().join("plugins/data/example");
    present(&data.join("failing")).await;
    another_session(&mut fixture).await;
    let (_, second_turn) = submit(&fixture, false).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while model.requests.lock().unwrap().len() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("second session bind deadline");
    fs::write(data.join("release_error"), "ready").unwrap();
    for (session, turn) in [(first, first_turn.id), (fixture.session.id, second_turn.id)] {
        let page = finished(&fixture.client, session, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        let values = results(&fixture.client, &page).await;
        assert_eq!(
            values.last().unwrap()["structuredContent"]["value"],
            "complete"
        );
    }
    assert_eq!(requests(&data, "initialize"), 1);
    assert_eq!(requests(&data, "tools/call"), 4);
    super::runtime::state(&fixture, Some(plugin::mcp::RuntimeState::Connected)).await;
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn isolates_stop_during_shared_elicitation() {
    use question::Response;
    use std::sync::atomic::Ordering;

    let peer = input::Server::url("https://example.invalid/continue", false).await;
    let model = Server::turn_tools(vec![(alias("input", "read"), json!({}))]).await;
    let mut fixture = process::Fixture::packages(&model, &[]).await;
    peer.package(&fixture.root.join("package"));
    install(&fixture, 0).await;
    let first = fixture.session.id;
    let (_, first_turn) = submit(&fixture, false).await;
    let (_, first_question) = questions::pending(&fixture.client, first).await;
    another_session(&mut fixture).await;
    let (_, second_turn) = submit(&fixture, false).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while model.requests.lock().unwrap().len() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("second session bind deadline");
    execute(
        &fixture.client,
        Command::StopTurn {
            turn: first_turn.id,
        },
    )
    .await;
    let (_, second_question) = questions::pending(&fixture.client, fixture.session.id).await;
    assert_eq!(second_question.turn, second_turn.id);
    assert_ne!(second_question.id, first_question.id);
    execute(
        &fixture.client,
        Command::ResolveQuestion {
            session: fixture.session.id,
            question: second_question.id,
            response: Response::Decline,
        },
    )
    .await;
    let cancelled = finished(&fixture.client, first, first_turn.id).await;
    let completed = finished(&fixture.client, fixture.session.id, second_turn.id).await;
    assert_eq!(cancelled.runs[0].status, Status::Cancelled);
    assert_eq!(completed.runs[0].status, Status::Completed, "{completed:?}");
    assert_eq!(peer.initializations.load(Ordering::SeqCst), 1);
    assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
    super::runtime::state(&fixture, Some(plugin::mcp::RuntimeState::Connected)).await;
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}

pub(super) async fn warmed(data: &Path, connections: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if data.join("requests.jsonl").is_file()
                && requests(data, "tools/list") >= connections * 2
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("MCP warm-up deadline");
}

#[tokio::test]
async fn warms_enabled_servers() {
    let model = Server::turn_tools(vec![(alias("native", "read"), json!({}))]).await;
    let fixture = process::Fixture::packages(&model, &[]).await;
    package(
        &fixture.root,
        json!({"native": peer::config("mcp::peer::stdio_peer", "normal")}),
        "warm",
    );
    let installed = install(&fixture, 0).await;
    let data = fixture.node.profile().join("plugins/data/example");
    warmed(&data, 1).await;
    assert!(model.requests.lock().unwrap().is_empty());
    assert_eq!(requests(&data, "initialize"), 1);
    assert_eq!(requests(&data, "tools/call"), 0);
    let (_, turn) = submit(&fixture, false).await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(page.runs[0].status, Status::Completed, "{page:?}");
    assert_eq!(requests(&data, "initialize"), 1);
    assert_eq!(requests(&data, "tools/list"), 2);
    let Output::Plugin(disabled) = execute(
        &fixture.client,
        Command::SetPluginEnabled {
            name: installed.summary.name,
            expected_revision: installed.summary.revision,
            enabled: false,
        },
    )
    .await
    else {
        panic!("plugin expected")
    };
    execute(
        &fixture.client,
        Command::SetPluginEnabled {
            name: disabled.summary.name,
            expected_revision: disabled.summary.revision,
            enabled: true,
        },
    )
    .await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while requests(&data, "tools/list") < 4 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("MCP enable warm-up deadline");
    assert_eq!(requests(&data, "initialize"), 2);
    assert_eq!(requests(&data, "tools/call"), 1);
    fixture.node.shutdown().await.unwrap();
    reaped(&data);
    fixture.controller.close().await.unwrap();
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn ignores_health_after_failed_warmup() {
    use crate::connector_fixture as connectors;
    use plugin::connector::{Input, Kind, State};
    use std::collections::BTreeMap;

    let model = Server::turn_tools(vec![]).await;
    let fixture = process::Fixture::packages(&model, &[]).await;
    let mut server = peer::config("mcp::peer::stdio_peer", "bad_handshake");
    server["env"]["SAILRY_MCP_TOKEN"] = json!("");
    package(&fixture.root, json!({"native":server}), "0.1.0");
    connectors::configure(
        &fixture.root.join("package"),
        json!({
            "id":"service","display":{"label":"Service"},"accounts":"single",
            "origins":[connectors::ORIGIN],"auth":[{"kind":"secret","fields":["token"]}],
            "fields":[{"name":"token","title":"Token","secret":true}],
            "inject":[{"server":"native","env":"SAILRY_MCP_TOKEN","field":"token"}]
        }),
    );
    let installed = install(&fixture, 0).await;
    assert!(installed.issues.is_empty(), "{:?}", installed.issues);
    let row = connectors::connected(
        &fixture.client,
        fixture.client.prepare(Command::StartConnectorLogin {
            package: installed.summary.reference(),
            connector: "service".into(),
            connection: None,
            expected_revision: None,
            method: Kind::Secret,
            input: Input {
                secrets: BTreeMap::from([(
                    "token".into(),
                    Secret::new("private-warm-credential".into()),
                )]),
                ..Default::default()
            },
        }),
    )
    .await;
    let data = fixture.node.profile().join("plugins/data/example");
    present(&data.join("pid")).await;
    closed(&data).await;
    assert_eq!(requests(&data, "initialize"), 1);
    for _ in 0..3 {
        let Output::Connection(checked) = execute(
            &fixture.client,
            Command::ProbeConnection { connection: row.id },
        )
        .await
        else {
            panic!("connection expected")
        };
        assert_eq!(checked.state, State::Connected);
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(requests(&data, "initialize"), 1);
    assert_eq!(fs::read(data.join("launches")).unwrap(), b"x");
    assert!(model.requests.lock().unwrap().is_empty());
    fixture.node.shutdown().await.unwrap();
    reaped(&data);
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn reconnects_closed_connections_once() {
    let model = Server::turn_tools(vec![(alias("native", "read"), json!({}))]).await;
    let fixture = process::Fixture::packages(&model, &[]).await;
    package(
        &fixture.root,
        json!({"native": peer::config("mcp::peer::stdio_peer", "close_after_call")}),
        "closing",
    );
    install(&fixture, 0).await;
    let data = fixture.node.profile().join("plugins/data/example");
    warmed(&data, 1).await;
    for expected in 1..=2 {
        let (_, turn) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert_eq!(requests(&data, "initialize"), expected);
        assert_eq!(requests(&data, "tools/list"), expected * 2);
        assert_eq!(requests(&data, "tools/call"), expected);
        assert_eq!(
            results(&fixture.client, &page).await.last().unwrap()["structuredContent"]["version"],
            "closing"
        );
    }
    fixture.node.shutdown().await.unwrap();
    reaped(&data);
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn routes_shared_elicitation_to_its_turn() {
    use question::{Answer, Response};
    use std::sync::atomic::Ordering;

    let peer = input::Server::url("https://example.invalid/continue", false).await;
    let model = Server::turn_tools(vec![(alias("input", "read"), json!({}))]).await;
    let mut fixture = process::Fixture::packages(&model, &[]).await;
    peer.package(&fixture.root.join("package"));
    install(&fixture, 0).await;
    let first = fixture.session.clone();
    let (_, first_turn) = submit(&fixture, false).await;
    let (_, first_question) = questions::pending(&fixture.client, first.id).await;
    let root = fixture.directory.path().join("other-project");
    fs::create_dir(&root).unwrap();
    let Output::Project(project) = execute(
        &fixture.client,
        Command::RegisterProject {
            name: "Other project".into(),
            path: root.to_str().unwrap().into(),
        },
    )
    .await
    else {
        panic!("project expected")
    };
    let Output::Session(second) = execute(
        &fixture.client,
        Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(first.config),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    assert_ne!(second.worktree, first.worktree);
    fixture.session = second;
    let (_, second_turn) = submit(&fixture, false).await;
    execute(
        &fixture.client,
        Command::ResolveQuestion {
            session: first.id,
            question: first_question.id,
            response: Response::Answer(Answer::Opened),
        },
    )
    .await;
    let (_, second_question) = questions::pending(&fixture.client, fixture.session.id).await;
    assert_eq!(first_question.turn, first_turn.id);
    assert_eq!(second_question.turn, second_turn.id);
    assert_ne!(first_question.id, second_question.id);
    execute(
        &fixture.client,
        Command::ResolveQuestion {
            session: fixture.session.id,
            question: second_question.id,
            response: Response::Decline,
        },
    )
    .await;
    let first = finished(&fixture.client, first.id, first_turn.id).await;
    let second = finished(&fixture.client, fixture.session.id, second_turn.id).await;
    assert_eq!(first.runs[0].status, Status::Completed, "{first:?}");
    assert_eq!(second.runs[0].status, Status::Completed, "{second:?}");
    assert_eq!(first.questions.len(), 1);
    assert_eq!(second.questions.len(), 1);
    assert_eq!(peer.initializations.load(Ordering::SeqCst), 1);
    assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}
