//! Controller metadata stays passive; explicit diagnostics own their temporary connection.
use super::*;
use plugin::mcp::Availability;

async fn read(client: &Client, name: &str) -> plugin::Info {
    let Output::Plugin(info) = execute(client, Command::ReadPlugin { name: name.into() }).await
    else {
        panic!("plugin expected")
    };
    info
}

#[tokio::test]
async fn checks_only_on_request() {
    for remote in [false, true] {
        let model = Server::start(false).await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        package(
            &fixture.root,
            json!({
                "native":peer::config("mcp::peer::stdio_peer", "normal"),
                "other":peer::config("mcp::peer::stdio_peer", "normal")
            }),
            "0.1.0",
        );
        let installed = install(&fixture, 0).await;
        let original = installed.summary.reference();
        let data = fixture.node.profile().join("plugins/data/example");
        assert!(installed.mcp.iter().all(|server| server.allowed));
        assert!(
            read(&fixture.client, "example")
                .await
                .mcp
                .iter()
                .all(|server| server.availability == Availability::Available)
        );
        execute(
            &fixture.client,
            Command::ReadPluginMcp {
                package: original.clone(),
            },
        )
        .await;
        let mut feed = fixture.client.subscribe_connections().await.unwrap();
        assert!(matches!(
            feed.next().await.unwrap(),
            Update::ConnectionsChanged(_)
        ));
        assert!(
            !data.join("pid").exists(),
            "directory reads must not launch MCP"
        );

        let request = fixture.client.prepare(Command::SetPluginMcpAllowed {
            package: original.clone(),
            server: "native".into(),
            allowed: false,
        });
        let admission = fixture.client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let Output::Plugin(disabled) = admission.completion.await.unwrap().unwrap() else {
            panic!("plugin expected")
        };
        assert!(disabled.summary.enabled);
        assert!(disabled.summary.settings_revision > original.settings_revision);
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::Plugin(disabled.clone())
        );
        assert!(
            !disabled
                .mcp
                .iter()
                .find(|server| server.name == "native")
                .unwrap()
                .allowed
        );
        assert!(
            disabled
                .mcp
                .iter()
                .find(|server| server.name == "other")
                .unwrap()
                .allowed
        );
        assert_eq!(
            disabled
                .mcp
                .iter()
                .find(|server| server.name == "native")
                .unwrap()
                .availability,
            Availability::ServerDisabled
        );
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::SetPluginMcpAllowed {
                    package: original.clone(),
                    server: "other".into(),
                    allowed: false,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let Output::Plugin(captured) = execute(
            &fixture.client,
            Command::ReadPluginVersion { package: original },
        )
        .await
        else {
            panic!("captured plugin expected")
        };
        assert!(captured.mcp.iter().all(|server| server.allowed));

        let check = fixture.client.prepare(Command::CheckPluginMcp {
            package: disabled.summary.reference(),
            server: "native".into(),
        });
        let admission = fixture.client.dispatch(check.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let output = admission.completion.await.unwrap().unwrap();
        let Output::PluginMcpChecked(checked) = &output else {
            panic!("MCP check expected")
        };
        assert_eq!(checked.package, disabled.summary.reference());
        assert_eq!(checked.server, "native");
        assert_eq!(
            checked
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            ["read", "write", "hold", "fail"]
        );
        reaped(&data);
        assert_eq!(fixture.client.execute(check).await.unwrap(), output);
        assert_eq!(
            fs::read_to_string(data.join("launches")).unwrap(),
            "x",
            "only the selected diagnostic may run"
        );
        assert!(model.requests.lock().unwrap().is_empty());
        assert_eq!(read(&fixture.client, "example").await, disabled);
        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: disabled.summary.name.clone(),
                expected_revision: disabled.summary.revision,
                enabled: false,
            },
        )
        .await;
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::CheckPluginMcp {
                    package: disabled.summary.reference(),
                    server: "native".into(),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "x");
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn freezes_server_choices() {
    for remote in [false, true] {
        let native = alias("native", "read");
        let model = Server::available_tools(vec![(native.clone(), json!({}))]).await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        isolate(&fixture.client).await;
        package(
            &fixture.root,
            json!({"native":peer::config("mcp::peer::stdio_peer", "normal")}),
            "0.1.0",
        );
        let installed = install(&fixture, 0).await;
        let (_, queued) = submit(&fixture, true).await;
        let Output::Plugin(disabled) = execute(
            &fixture.client,
            Command::SetPluginMcpAllowed {
                package: installed.summary.reference(),
                server: "native".into(),
                allowed: false,
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        assert_eq!(queued.plugins, [installed.summary.reference()]);
        execute(
            &fixture.client,
            Command::StartQueuedTurn { turn: queued.id },
        )
        .await;
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(
            results(&page).len(),
            1,
            "admitted work retains its configuration"
        );
        let previous = model.requests.lock().unwrap().len();
        let (_, next) = submit(&fixture, false).await;
        assert_eq!(next.plugins, [disabled.summary.reference()]);
        let page = finished(&fixture.client, fixture.session.id, next.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        for request in &model.requests.lock().unwrap()[previous..] {
            assert!(
                request["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .all(|tool| tool["function"]["name"] != native)
            );
        }
        let data = fixture.node.profile().join("plugins/data/example");
        assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "x");
        reaped(&data);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn closes_failed_checks() {
    for remote in [false, true] {
        let model = Server::start(false).await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        package(
            &fixture.root,
            json!({"native":peer::config("mcp::peer::stdio_peer", "bad_handshake")}),
            "0.1.0",
        );
        let installed = install(&fixture, 0).await;
        let request = fixture.client.prepare(Command::CheckPluginMcp {
            package: installed.summary.reference(),
            server: "native".into(),
        });
        let error = fixture.client.execute(request.clone()).await.unwrap_err();
        let data = fixture.node.profile().join("plugins/data/example");
        reaped(&data);
        assert_eq!(fixture.client.execute(request).await.unwrap_err(), error);
        assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "x");
        assert_eq!(read(&fixture.client, "example").await, installed);
        assert!(model.requests.lock().unwrap().is_empty());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
