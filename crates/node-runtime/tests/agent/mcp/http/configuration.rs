use super::*;
use sailry_protocol::plugin::mcp::{Server as McpServer, State};

async fn read(client: &Client, package: plugin::Reference) -> State {
    let Output::PluginMcp(state) = execute(client, Command::ReadPluginMcp { package }).await else {
        panic!("MCP configuration expected")
    };
    state
}

#[tokio::test]
async fn preserves_bound_configuration() {
    use plugin::settings::SecretUpdate;

    let http = Http::start(Reply::Normal).await;
    let model = Server::turn_tools(vec![(
        alias("remote", "write"),
        json!({"value":"approved"}),
    )])
    .await;
    let fixture = process::Fixture::packages(&model, &[]).await;
    package(
        &fixture.root,
        json!({"remote":{"type":"streamable-http","url":http.endpoint,"headers":{"Authorization":""}}}),
        "bound",
    );
    declare_token(&fixture.root.join("package"));
    let installed = install(&fixture, 0).await;
    let original = "Bearer bound-original-private";
    let replacement = "Bearer bound-replacement-private";
    let configured = save_settings(
        &fixture.client,
        installed.summary.reference(),
        "Original",
        SecretUpdate::Replace(Secret::new(original.into())),
    )
    .await;
    http.listed(1).await;
    assert!(model.requests.lock().unwrap().is_empty());
    let (_, turn) = submit(&fixture, false).await;
    let (_, approval) = crate::approvals::pending(&fixture.client, fixture.session.id).await;
    assert_eq!(approval.turn, turn.id);
    assert_eq!(http.count("initialize"), 1);
    let updated = save_settings(
        &fixture.client,
        configured,
        "Updated",
        SecretUpdate::Replace(Secret::new(replacement.into())),
    )
    .await;
    http.listed(2).await;
    assert_eq!(http.count("tools/call"), 0);
    execute(
        &fixture.client,
        Command::ResolveApproval {
            session: fixture.session.id,
            approval: approval.id,
            decision: Decision::Approve,
        },
    )
    .await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{page:?}"
    );
    assert_eq!(
        results(&fixture.client, &page).await.last().unwrap()["structuredContent"]["value"],
        "approved"
    );
    http.closed(1).await;
    let (_, next) = submit(&fixture, false).await;
    assert!(next.plugins.contains(&updated));
    let (_, approval) = crate::approvals::pending(&fixture.client, fixture.session.id).await;
    assert_eq!(approval.turn, next.id);
    execute(
        &fixture.client,
        Command::ResolveApproval {
            session: fixture.session.id,
            approval: approval.id,
            decision: Decision::Approve,
        },
    )
    .await;
    let page = finished(&fixture.client, fixture.session.id, next.id).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{page:?}"
    );
    assert_eq!(
        results(&fixture.client, &page).await.last().unwrap()["structuredContent"]["value"],
        "approved"
    );
    assert_eq!(http.count("initialize"), 2);
    let tokens = http
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, _, body)| body["method"] == "tools/call")
        .map(|(_, headers, _)| headers["authorization"].clone())
        .collect::<Vec<_>>();
    assert_eq!(tokens, [original, replacement]);
    for secret in [original, replacement] {
        assert!(!serde_json::to_string(&page).unwrap().contains(secret));
        assert!(
            !serde_json::to_string(&*model.requests.lock().unwrap())
                .unwrap()
                .contains(secret)
        );
    }
    fixture.node.shutdown().await.unwrap();
    assert_eq!(http.deletes(), 2);
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn executes_frozen_configuration() {
    for header in ["Authorization", "X-Api-Key"] {
        let name = "sample";
        let original_peer = Http::start(Reply::Normal).await;
        let replacement_peer = Http::start(Reply::Normal).await;
        let model = Server::turn_tools(vec![(
            peer::package_alias("example", name, "read"),
            json!({}),
        )])
        .await;
        let fixture = process::Fixture::packages(&model, &[]).await;
        package(
            &fixture.root,
            json!({name:{"type":"streamable-http","url":"https://example.invalid/mcp","headers":{}}}),
            "0.1.0",
        );
        let installed = install(&fixture, 0).await;
        let initial = read(&fixture.client, installed.summary.reference()).await;
        let mut configuration = initial.configuration;
        let McpServer::Http { url, headers, .. } = configuration.servers.get_mut(name).unwrap()
        else {
            unreachable!()
        };
        let original_endpoint = url.clone();
        *url = original_peer.endpoint.clone();
        let original_secret = if header == "Authorization" {
            "Bearer original-${PLUGIN_ROOT}-token"
        } else {
            "original-${PLUGIN_ROOT}-key"
        };
        headers.insert(header.into(), original_secret.into());
        let request = fixture.client.prepare(Command::SavePluginMcp {
            package: initial.package,
            configuration: configuration.clone(),
        });
        let Output::Plugin(first) = fixture.client.execute(request.clone()).await.unwrap() else {
            panic!("plugin expected")
        };
        original_peer.listed(1).await;
        assert_eq!(original_peer.count("initialize"), 1);
        assert_eq!(original_peer.count("tools/call"), 0);
        let (_, queued) = submit(&fixture, true).await;
        assert!(queued.plugins.contains(&first.summary.reference()));
        let McpServer::Http { url, headers, .. } = configuration.servers.get_mut(name).unwrap()
        else {
            unreachable!()
        };
        *url = replacement_peer.endpoint.clone();
        let next_secret = if header == "Authorization" {
            "Bearer replacement-${PLUGIN_DATA}-token"
        } else {
            "replacement-${PLUGIN_DATA}-key"
        };
        headers.insert(header.into(), next_secret.into());
        let Output::Plugin(latest) = execute(
            &fixture.client,
            Command::SavePluginMcp {
                package: first.summary.reference(),
                configuration,
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        replacement_peer.listed(1).await;
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::Plugin(first)
        );
        execute(
            &fixture.client,
            Command::StartQueuedTurn { turn: queued.id },
        )
        .await;
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        assert_eq!(original_peer.count("initialize"), 2);
        assert_eq!(original_peer.count("tools/call"), 1);
        assert_eq!(replacement_peer.count("initialize"), 1);
        assert_eq!(replacement_peer.count("tools/call"), 0);
        let (_, next) = submit(&fixture, false).await;
        assert!(next.plugins.contains(&latest.summary.reference()));
        let page = finished(&fixture.client, fixture.session.id, next.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        assert_eq!(replacement_peer.count("initialize"), 1);
        assert_eq!(replacement_peer.count("tools/call"), 1);
        for (peer, value) in [
            (&original_peer, original_secret),
            (&replacement_peer, next_secret),
        ] {
            for (_, headers, _) in peer.requests.lock().unwrap().iter() {
                assert_eq!(
                    headers
                        .get(&header.to_ascii_lowercase())
                        .map(String::as_str),
                    Some(value)
                );
            }
        }
        let projected = serde_json::to_string(&page).unwrap();
        let model_requests = serde_json::to_string(&*model.requests.lock().unwrap()).unwrap();
        for secret in [original_secret, next_secret] {
            assert!(!projected.contains(secret));
            assert!(!model_requests.contains(secret));
        }
        let immutable = fixture
            .node
            .profile()
            .join("plugins/packages")
            .join(&latest.summary.digest)
            .join("mcp.json");
        assert!(
            fs::read_to_string(immutable)
                .unwrap()
                .contains(&original_endpoint)
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
