use super::*;
use serde_json::json;
use std::collections::BTreeMap;
#[cfg(feature = "test-support")]
#[path = "http/cancellation.rs"]
mod cancellation;
#[path = "http/revisions.rs"]
mod revisions;
#[path = "../../plugins/http/server.rs"]
pub(super) mod server;

impl server::Server {
    async fn with_json(body: serde_json::Value, held_path: Option<String>) -> Self {
        Self::respond(Some(body.to_string()), held_path).await
    }
}

const PAT: &str = "figma-pat-private-marker";

async fn install_figma(fixture: &process::Fixture, origin: &str, health: bool) {
    let root = fixture.root.join("figma-fixture");
    std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
    let manifest = include_str!("../../../../../plugins/figma/plugin.json");
    let manifest = if health {
        let mut manifest: serde_json::Value = serde_json::from_str(manifest).unwrap();
        let declaration = &mut manifest["extensions"]["dev.sailry.platform"]["connectors"][0];
        declaration["fields"].as_array_mut().unwrap().extend([
            json!({"name":"tenant","title":"Tenant"}),
            json!({"name":"label","title":"Label"}),
        ]);
        declaration["health"] =
            json!({"request":"https://api.figma.com/{tenant}/health","ok":[200]});
        manifest.to_string()
    } else {
        manifest.into()
    };
    let source = include_str!("../../../../../plugins/figma/dev.sailry.platform/host/main.js");
    fixture
        .node
        .set_connector_fixture("https://api.figma.com", origin);
    std::fs::write(root.join("plugin.json"), manifest).unwrap();
    std::fs::write(root.join("dev.sailry.platform/host/main.js"), source).unwrap();
    let Output::Plugin(info) = fixture
        .client
        .execute(fixture.client.prepare(Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "figma-fixture".into(),
            name: "figma".into(),
            expected_revision: 0,
            expected_digest: None,
        }))
        .await
        .unwrap()
    else {
        panic!("Figma package expected");
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    let Output::ConnectorLogin(attempt) = fixture
        .client
        .execute(fixture.client.prepare(Command::StartConnectorLogin {
            package: info.summary.reference(),
            connector: "figma".into(),
            connection: None,
            expected_revision: None,
            method: sailry_protocol::plugin::connector::Kind::Secret,
            input: sailry_protocol::plugin::connector::Input {
                fields: if health {
                    BTreeMap::from([
                        ("tenant".into(), "current".into()),
                        ("label".into(), "Original".into()),
                    ])
                } else {
                    BTreeMap::new()
                },
                secrets: BTreeMap::from([("token".into(), Secret::new(PAT.into()))]),
                ..Default::default()
            },
        }))
        .await
        .unwrap()
    else {
        panic!("connection attempt expected");
    };
    let mut progress = fixture
        .client
        .subscribe_connector_login(attempt.id)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let Update::ConnectorLogin(update) = progress.next().await.unwrap() else {
                panic!("connection update expected");
            };
            assert!(!serde_json::to_string(&update).unwrap().contains(PAT));
            match update.state {
                sailry_protocol::plugin::connector::LoginState::Connected(_) => break,
                sailry_protocol::plugin::connector::LoginState::Failed(error) => {
                    panic!("{error:?}")
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn reads_figma_with_private_pat_and_approval() {
    for remote in [false, true] {
        let data = json!({"name":"Actual file","version":"9007199254740993",
            "document":{"id":"0:0","children":[]},"nodes":{"1:2":null}});
        let http = server::Server::with_json(data.clone(), None).await;
        let name = plugin_tool("figma", "read");
        let model = Server::tools(vec![
            (name.clone(), json!({"action":"file","file_key":"file-key","version":"9007199254740993","depth":2})),
            (name.clone(), json!({"action":"file","file_key":"denied-file"})),
            (name.clone(), json!({"action":"nodes","file_key":"file-key","ids":["1:2"]})),
        ]).await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        install_figma(&fixture, &http.origin, false).await;
        let mut updates = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        let mut projection = sailry_client::conversation::Projection::new(
            fixture.client.target(),
            fixture.session.id,
            0,
        );
        projection.apply(0, updates.next().await.unwrap()).unwrap();
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        for (index, decision) in [Decision::Approve, Decision::Deny, Decision::Approve]
            .into_iter()
            .enumerate()
        {
            let (page, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
            assert!(!serde_json::to_string(&page).unwrap().contains(PAT));
            assert_eq!(http.requests.lock().unwrap().len(), usize::from(index > 0));
            process::decide(&fixture.client, &approval, decision).await;
        }
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn)
                .unwrap()
                .status,
            Status::Completed
        );
        let results: Vec<_> = tool_details(&fixture.client, &page)
            .await
            .into_iter()
            .filter(|(actual, _)| actual == &name)
            .map(|(_, detail)| detail.result.unwrap())
            .collect();
        assert_eq!(results.len(), 3, "{results:?}");
        assert_eq!(results[0]["data"], data);
        assert_eq!(results[0]["untrusted"], true);
        assert_eq!(results[0]["partial"], true);
        assert_eq!(results[0]["depth"], 2);
        assert_eq!(
            results[1],
            serde_json::to_value(rmcp::model::CallToolResult::error(vec![
                rmcp::model::ContentBlock::text("Tool execution was denied."),
            ]))
            .unwrap()
        );
        assert_eq!(results[2]["data"]["nodes"]["1:2"], serde_json::Value::Null);
        assert_eq!(results[2]["partial"], true);
        {
            let requests = http.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(requests[0].0.starts_with(
                "GET /v1/files/file-key?version=9007199254740993&depth=2 HTTP/1.1\r\n"
            ));
            assert!(
                requests[1]
                    .0
                    .starts_with("GET /v1/files/file-key/nodes?ids=1%3A2 HTTP/1.1\r\n")
            );
            for (header, body) in requests.iter() {
                assert!(header.contains(&format!("x-figma-token: {PAT}\r\n")));
                assert!(!header.contains("Bearer "));
                assert!(body.is_empty());
            }
        }
        assert!(!serde_json::to_string(&page).unwrap().contains(PAT));
        assert!(
            !serde_json::to_string(&*model.requests.lock().unwrap())
                .unwrap()
                .contains(PAT)
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let update = updates.next().await.unwrap();
                assert!(!serde_json::to_string(&update).unwrap().contains(PAT));
                projection.apply(0, update).unwrap();
                if projection
                    .snapshot()
                    .unwrap()
                    .page
                    .runs
                    .iter()
                    .any(|run| run.turn == turn && run.status == Status::Completed)
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        let mut reconnected = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        assert!(
            !serde_json::to_string(&reconnected.next().await.unwrap())
                .unwrap()
                .contains(PAT)
        );
        let snapshot = fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap();
        assert!(!serde_json::to_string(&snapshot).unwrap().contains(PAT));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn stopping_figma_read_closes_the_request() {
    for remote in [false, true] {
        let http = server::Server::with_json(
            json!({"document":{"id":"0:0"}}),
            Some("/v1/files/held-file".into()),
        )
        .await;
        let model = Server::tools(vec![(
            plugin_tool("figma", "read"),
            json!({"action":"file","file_key":"held-file"}),
        )])
        .await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        install_figma(&fixture, &http.origin, false).await;
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let (page, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert!(!serde_json::to_string(&page).unwrap().contains(PAT));
        assert!(http.requests.lock().unwrap().is_empty());
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        tokio::time::timeout(Duration::from_secs(5), http.held.cancelled())
            .await
            .unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::StopTurn { turn }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn)
                .unwrap()
                .status,
            Status::Cancelled
        );
        tokio::time::timeout(Duration::from_secs(3), http.disconnected.cancelled())
            .await
            .unwrap();
        assert_eq!(http.requests.lock().unwrap().len(), 1);
        assert!(!serde_json::to_string(&page).unwrap().contains(PAT));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn applies_approval_and_turn_cancellation() {
    for remote in [false, true] {
        for cancel in [false, true] {
            let http = server::Server::start().await;
            let digest = blake3::hash(b"http-fixture").to_hex();
            let name = format!("plugin_{}_request", &digest[..16]);
            let model = Server::tools(vec![(
                name,
                json!({"method":"POST",
                    "url": format!("{}{}", http.origin, if cancel {"/hold"} else {"/ok"}),
                    "body":"from agent", "timeout_ms":60000
                }),
            )])
            .await;
            let fixture = process::Fixture::packages(remote, &model, &[]).await;
            std::fs::create_dir(fixture.root.join("package")).unwrap();
            std::fs::write(
                fixture.root.join("package/plugin.json"),
                json!({
                    "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                    "name":"http-fixture", "version":"0.1.0", "extensions":{"dev.sailry.platform":{
                        "api_version":"v1", "actions":["http.request"],
                        "tools":[{"name":"request", "operation":"http.request"}]
                    }}
                })
                .to_string(),
            )
            .unwrap();
            let Output::Plugin(info) = fixture
                .client
                .execute(fixture.client.prepare(Command::InstallPlugin {
                    worktree: fixture.session.worktree,
                    path: "package".into(),
                    name: "http-fixture".into(),
                    expected_revision: 0,
                    expected_digest: None,
                }))
                .await
                .unwrap()
            else {
                panic!("plugin expected")
            };
            assert!(info.issues.is_empty(), "{:?}", info.issues);
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: "Send the request".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
            assert!(http.requests.lock().unwrap().is_empty());
            process::decide(
                &fixture.client,
                &approval,
                sailry_protocol::conversation::Decision::Approve,
            )
            .await;
            if cancel {
                tokio::time::timeout(Duration::from_secs(5), http.held.cancelled())
                    .await
                    .unwrap();
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::StopTurn { turn: turn.id }))
                    .await
                    .unwrap();
            }
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs[0].status,
                if cancel {
                    Status::Cancelled
                } else {
                    Status::Completed
                },
                "{:?}",
                page.runs
            );
            if cancel {
                tokio::time::timeout(Duration::from_secs(3), http.disconnected.cancelled())
                    .await
                    .unwrap();
            } else {
                assert!(
                    tool_details(&fixture.client, &page)
                        .await
                        .iter()
                        .any(|(_, detail)| {
                            let result = detail.result.as_ref().unwrap();
                            result["status"] == 201 && result["body"] == "saved"
                        })
                );
            }
            assert_eq!(http.requests.lock().unwrap().len(), 1);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
