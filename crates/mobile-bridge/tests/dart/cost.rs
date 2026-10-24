use super::configuration::{config, provider, support};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::*;

#[path = "../../../node-runtime/tests/support/pricing.rs"]
mod pricing;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn restores_partial_costs_across_nodes() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let model = runtime.block_on(support::Server::start(false));
    let mut nodes = Vec::new();
    for count in [1, 2] {
        let profile = directory.path().join(format!("node-{count}"));
        let root = directory.path().join(format!("project-{count}"));
        std::fs::create_dir(&root).unwrap();
        let node = runtime.block_on(async {
            let node = Node::start(&profile).await.unwrap();
            let client = Client::new(node.local());
            let mut selected = provider(&client, &model.endpoint, "priced-fixture").await;
            let mut unpriced = selected.models[0].clone();
            unpriced.id = "unpriced-fixture".into();
            selected.models.push(unpriced);
            let Output::Provider(selected) = client
                .execute(client.prepare(Command::PutProvider {
                    expected_revision: selected.revision,
                    provider: selected,
                }))
                .await
                .unwrap()
            else {
                panic!("provider expected")
            };
            let Output::Project(project) = client
                .execute(client.prepare(Command::RegisterProject {
                    name: "Usage fixture".into(),
                    path: root.to_str().unwrap().into(),
                }))
                .await
                .unwrap()
            else {
                panic!("project expected")
            };
            let Output::Session(mut session) = client
                .execute(client.prepare(Command::CreateSession {
                    project: Some(project.id),
                    worktree: None,
                    config: Some(config(&selected)),
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            for index in 0..count {
                if index == 1 {
                    let mut config = session.config.clone();
                    config.model = "unpriced-fixture".into();
                    let Output::Session(updated) = client
                        .execute(client.prepare(Command::SetSessionConfig {
                            session: session.id,
                            expected_revision: session.revision,
                            config,
                        }))
                        .await
                        .unwrap()
                    else {
                        panic!("session expected")
                    };
                    session = updated;
                }
                let Output::QueuedTurn(turn) = client
                    .execute(client.prepare(Command::SubmitTurn {
                        session: session.id,
                        expected_revision: session.revision,
                        message: "Report usage".into(),
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("turn expected")
                };
                tokio::time::timeout(std::time::Duration::from_secs(10), async {
                    loop {
                        let Output::Conversation(history) = client
                            .execute(client.prepare(Command::ReadConversation {
                                session: session.id,
                                before: None,
                                limit: 20,
                            }))
                            .await
                            .unwrap()
                        else {
                            panic!("history expected")
                        };
                        if history.page.runs.iter().any(|run| {
                            run.turn == turn.id && run.status == conversation::Status::Completed
                        }) {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
            }
            node.shutdown().await.unwrap();
            pricing::seed(&profile).await;
            seed_catalog(&profile).await;
            Node::start(&profile).await.unwrap()
        });
        nodes.push(node);
    }
    let first = nodes[0].link().invite().unwrap();
    let second = nodes[1].link().invite().unwrap();
    let child = super::command("cost.dart", directory.path(), first.ticket())
        .env("SAILRY_SECOND_INVITATION", second.ticket())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    for node in nodes {
        runtime.block_on(node.shutdown()).unwrap();
    }
    assert!(status.success());
    assert_eq!(model.requests.lock().unwrap().len(), 3);
}

async fn seed_catalog(profile: &std::path::Path) {
    let (database, db) =
        crate::database_fixture::open(&profile.join("storage/node.turso"), false).await;
    db.execute(
        "INSERT INTO model_catalog(provider,id,body) VALUES('openai',?1,?2)",
        turso::params![
            "priced-fixture",
            serde_json::to_vec(&serde_json::json!({
                "id": "priced-fixture",
                "cost": {"input":1,"output":1,"cache_read":1,"cache_write":1}
            }))
            .unwrap()
        ],
    )
    .await
    .unwrap();
    crate::database_fixture::close(database, db).await;
}
