use super::*;
use serde_json::json;

#[tokio::test]
async fn recovers_without_replaying_work() {
    for remote in [false, true] {
        for stopped in [false, true] {
            let server = Server::tools(vec![(
                plugin_tool("files", "write_file"),
                json!({"path":"written.txt", "text":"Written once", "expected_revision":null}),
            )])
            .await;
            let mut fixture = Fixture::with_server(remote, server).await;
            let mut provider = fixture.provider.clone();
            provider
                .models
                .iter_mut()
                .for_each(|model| model.tools = true);
            fixture
                .client
                .execute(fixture.client.prepare(Command::PutProvider {
                    expected_revision: provider.revision,
                    provider,
                }))
                .await
                .unwrap();
            let mut config = fixture.session.config.clone();
            config.permission = Permission::Full;
            let Output::Session(session) = fixture
                .client
                .execute(fixture.client.prepare(Command::SetSessionConfig {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    config,
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            fixture.session = session;
            fixture
                .node
                .with_storage(|db| {
                    Box::pin(async move {
                        db.execute_batch(
                            "CREATE TRIGGER reject_finish BEFORE UPDATE ON agent_runs
                  WHEN json_type(NEW.body,'$.finished_ms')='integer'
                  AND json_type(OLD.body,'$.finished_ms') IS NOT 'integer'
                  BEGIN SELECT RAISE(ABORT,'settlement unavailable'); END;",
                        )
                        .await
                        .map_err(crate::database_fixture::fault)
                    })
                })
                .await
                .unwrap();
            let active = fixture.push("Write the file once").await;
            fixture.server.wait_count(2).await;
            let waiting = fixture.enqueue("Continue after storage recovers").await;
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let page = history(&fixture.client, fixture.session.id).await;
                    if page.entries.iter().filter(|entry| entry.turn == active.id)
                        .flat_map(|entry| &entry.parts)
                        .any(|part| matches!(part, Part::Text(text) if text.contains("answer-fixture"))) { break; }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }).await.expect("canonical answer deadline");
            if stopped {
                fixture
                    .client
                    .execute(
                        fixture
                            .client
                            .prepare(Command::StopTurn { turn: active.id }),
                    )
                    .await
                    .unwrap();
            }
            // Keep the storage fault across more than one settlement attempt.
            tokio::time::sleep(Duration::from_millis(1100)).await;
            let blocked = history(&fixture.client, fixture.session.id).await;
            assert_eq!(
                blocked
                    .runs
                    .iter()
                    .find(|run| run.turn == active.id)
                    .unwrap()
                    .status,
                if stopped {
                    Status::Stopping
                } else {
                    Status::Running
                }
            );
            assert_eq!(
                blocked
                    .runs
                    .iter()
                    .find(|run| run.turn == waiting.id)
                    .unwrap()
                    .status,
                Status::Queued
            );
            assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
            assert_eq!(
                std::fs::read_to_string(fixture.directory.path().join("project/written.txt"))
                    .unwrap(),
                "Written once"
            );
            fixture
                .node
                .with_storage(|db| {
                    Box::pin(async move {
                        db.execute_batch("DROP TRIGGER reject_finish")
                            .await
                            .map_err(crate::database_fixture::fault)
                    })
                })
                .await
                .unwrap();
            let recovered = finished(&fixture.client, fixture.session.id, active.id).await;
            assert_eq!(
                recovered
                    .runs
                    .iter()
                    .find(|run| run.turn == active.id)
                    .unwrap()
                    .status,
                if stopped {
                    Status::Cancelled
                } else {
                    Status::Completed
                }
            );
            if stopped {
                fixture.pause(false).await;
            }
            let page = finished(&fixture.client, fixture.session.id, waiting.id).await;
            assert!(page.queue.items.is_empty());
            assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
            assert_eq!(
                page.entries
                    .iter()
                    .filter(|entry| entry.turn == active.id)
                    .flat_map(|entry| &entry.parts)
                    .filter(|part| matches!(part, Part::ToolCall { .. }))
                    .count(),
                1
            );
            fixture.close().await;
        }
    }
}
