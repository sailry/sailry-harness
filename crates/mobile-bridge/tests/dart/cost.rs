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
            let selected = provider(&client, &model.endpoint, "priced-fixture").await;
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
            let Output::Session(session) = client
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
            for _ in 0..count {
                let Output::QueuedTurn(turn) = client
                    .execute(client.prepare(Command::SubmitTurn {
                        session: session.id,
                        expected_revision: 1,
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
            pricing::seed(&profile);
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
