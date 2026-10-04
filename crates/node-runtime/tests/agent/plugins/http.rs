use super::*;
use serde_json::json;
#[path = "../../plugins/http/server.rs"]
mod server;

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
            let fixture = process::Fixture::new(remote, &model).await;
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
                assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part,
                    Part::ToolResult { result, .. } if result["status"] == 201 && result["body"] == "saved"
                )));
            }
            assert_eq!(http.requests.lock().unwrap().len(), 1);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
