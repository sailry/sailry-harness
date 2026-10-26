use super::*;
use serde_json::json;

async fn install(fixture: &Fixture, fails: bool) {
    let root = fixture.root.join("preparation-fixture");
    std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/host/main.js"),
        format!(
            r#"
import {{getValue}} from 'sailry/sdk';
export async function initialize() {{
  await getValue('prepared');
  if ({fails}) throw new Error('preparation fixture failed');
  return {{instruction:'',tools:[]}};
}}
export function unused() {{return {{}};}}
"#
        ),
    )
    .unwrap();
    std::fs::write(root.join("plugin.json"), json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name":"preparation-fixture","version":"0.1.0",
        "extensions":{"dev.sailry.platform":{
            "api_version":"v1","actions":["storage.read"],
            "host":{"entry":"dev.sailry.platform/host/main.js",
                "resources":["dev.sailry.platform/host/main.js"],
                "handlers":["initialize","unused"],"turn":{"initialize":"initialize","before_model":null}},
            "tools":[{"name":"unused","description":"Exercise turn preparation",
                "handler":{"name":"unused","read_only":true,
                    "parameters":{"type":"object","properties":{},"additionalProperties":false}}}]
        }}
    }).to_string()).unwrap();
    let Output::Plugin(package) = execute(
        &fixture.client,
        Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "preparation-fixture".into(),
            name: "preparation-fixture".into(),
            expected_revision: 0,
            expected_digest: None,
        },
    )
    .await
    else {
        panic!("preparation package expected");
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
}

#[tokio::test]
async fn preconnects_each_turn() {
    for remote in [false, true] {
        for effort in [Effort::High, Effort::Max] {
            let started = std::time::Instant::now();
            let server = Server::transport(ModelApi::Responses, Reply::Text, Mode::Accept).await;
            let mut fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            fixture.provider.models[0].efforts = vec![effort];
            fixture.provider.models[0].default_effort = effort;
            execute(
                &fixture.client,
                Command::PutProvider {
                    expected_revision: fixture.provider.revision,
                    provider: fixture.provider.clone(),
                },
            )
            .await;
            let mut config = fixture.session.config.clone();
            config.effort = effort;
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
                panic!("session expected");
            };
            fixture.session = session;
            install(&fixture, false).await;
            // Preparation must not expire its own preconnection before generation starts.
            let lifetime = Duration::from_secs(5);
            fixture
                .node
                .set_model_transport_fixture(adk_model::openai::WsOptions {
                    idle_timeout: lifetime,
                    ..Default::default()
                });
            for index in 0..3 {
                let (entered, release) = fixture.node.hold_plugin_read_fixture();
                let turn = fixture.submit("Prepare the next generation").await;
                let request = tokio::time::timeout(Duration::from_secs(10), entered)
                    .await
                    .expect("turn preparation deadline")
                    .unwrap();
                assert!(matches!(request.command, Command::ReadPluginValue { .. }));
                tokio::time::timeout(Duration::from_secs(10), async {
                    while server.upgrades.lock().unwrap().len() < index + 1 {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap_or_else(|_| {
                    panic!("turn {index} must preconnect (remote={remote}, effort={effort:?})")
                });
                let upgrades = server.upgrades.lock().unwrap().len();
                let requests = server.requests.lock().unwrap().len();
                assert_eq!(
                    upgrades,
                    index + 1,
                    "remote={remote}, effort={effort:?}, turn={index}, requests={requests}, handshakes={:?}",
                    server
                        .upgrades
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|request| request.received.duration_since(started))
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    requests, index,
                    "remote={remote}, effort={effort:?}, turn={index}"
                );
                release.cancel();
                let mut page = finished(&fixture.client, fixture.session.id, turn).await;
                page.entries.retain(|entry| entry.turn == turn);
                assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                assert_eq!(completion::response(&page), server::ANSWER);
                assert!(super::super::retry::attempts(&page).is_empty());
                {
                    let requests = server.requests.lock().unwrap();
                    assert_eq!(requests.len(), index + 1);
                    assert_eq!(requests[index].body["type"], "response.create");
                    assert_eq!(requests[index].body["reasoning"]["effort"], json!(effort));
                }
                if index < 2 {
                    tokio::time::sleep(lifetime + Duration::from_millis(50)).await;
                }
            }
            assert_eq!(server.upgrades.lock().unwrap().len(), 3);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn owns_preconnections_during_preparation() {
    for remote in [false, true] {
        for outcome in ["released", "failed", "cancelled", "unused", "refused"] {
            let mode = if outcome == "refused" {
                Mode::Refuse(426)
            } else {
                Mode::Accept
            };
            let server = Server::transport(ModelApi::Responses, Reply::Text, mode).await;
            let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            install(&fixture, outcome == "failed").await;
            fixture
                .node
                .set_plugin_deadline_fixture(Duration::from_secs(35));
            let (entered, release) = fixture.node.hold_plugin_read_fixture();
            let turn = fixture.submit("Prepare before generating").await;
            let request = tokio::time::timeout(Duration::from_secs(10), entered)
                .await
                .expect("turn preparation deadline")
                .unwrap();
            assert!(matches!(request.command, Command::ReadPluginValue { .. }));
            tokio::time::timeout(Duration::from_secs(10), async {
                while server.upgrades.lock().unwrap().is_empty() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("preconnection handshake deadline");
            assert!(server.requests.lock().unwrap().is_empty());
            if outcome == "unused" {
                tokio::time::timeout(Duration::from_secs(30), async {
                    while server.closed.load(Ordering::SeqCst) == 0 {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .expect("unused preconnection must close within thirty seconds");
                assert!(server.requests.lock().unwrap().is_empty());
            }
            if outcome == "cancelled" {
                execute(&fixture.client, Command::StopTurn { turn }).await;
            } else {
                release.cancel();
            }
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            let expected = match outcome {
                "failed" => Status::Failed,
                "cancelled" => Status::Cancelled,
                _ => Status::Completed,
            };
            assert_eq!(
                page.runs.last().unwrap().status,
                expected,
                "remote={remote}, outcome={outcome}, {page:?}"
            );
            if outcome == "failed" {
                let fault = page.runs.last().unwrap().error.as_ref().unwrap();
                let Some(sailry_protocol::FaultDetails::SystemDiagnostic(report)) = &fault.details
                else {
                    panic!("safe callback diagnostic expected");
                };
                assert_eq!(fault.message, "plugin callback failed");
                let Output::Snapshot(snapshot) = execute(&fixture.client, Command::Snapshot).await
                else {
                    panic!("snapshot expected");
                };
                let notices: Vec<_> = snapshot.notifications.iter().filter(|notice| {
                    matches!(&notice.content.details, Some(sailry_protocol::notification::Details::System(existing))
                        if existing.incident == report.incident && existing.turn == Some(turn))
                }).collect();
                assert_eq!(notices.len(), 1);
                assert!(!snapshot.notifications.iter().any(|notice| {
                    matches!(&notice.content.details, Some(sailry_protocol::notification::Details::Task(task)) if task.turn == turn)
                }));
                assert!(
                    !serde_json::to_string(&snapshot.notifications)
                        .unwrap()
                        .contains("preparation fixture failed")
                );
            }
            if matches!(outcome, "failed" | "cancelled") {
                // ADK, the socket owner, closes the unused preconnection.
                tokio::time::timeout(Duration::from_secs(30), async {
                    while server.closed.load(Ordering::SeqCst) == 0 {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .expect("stopped preparation must release its preconnection");
                assert!(server.requests.lock().unwrap().is_empty());
            } else {
                assert_eq!(completion::response(&page), server::ANSWER);
                assert_eq!(server.requests.lock().unwrap().len(), 1);
                assert_eq!(
                    super::super::retry::attempts(&page),
                    if outcome == "refused" {
                        vec![1]
                    } else {
                        vec![]
                    }
                );
                assert_eq!(
                    server.requests.lock().unwrap()[0].body["type"] == "response.create",
                    outcome != "refused"
                );
            }
            assert_eq!(
                server.upgrades.lock().unwrap().len(),
                if outcome == "unused" { 2 } else { 1 }
            );
            release.cancel();
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}
