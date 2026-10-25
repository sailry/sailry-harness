use super::configuration::{config, provider, support};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output, conversation::Status};
use std::time::Duration;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn controls_owned_resources() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::start(false));
    let node = runtime.block_on(async {
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(node.local());
        let selected = provider(&client, &server.endpoint, "ffi-unassigned").await;
        client
            .execute(client.prepare(Command::SetDefaults {
                expected_revision: 0,
                config: config(&selected),
            }))
            .await
            .unwrap();
        node
    });
    let client = Client::new(node.local());
    let claimant = Client::new(node.local());
    let claimed = runtime.spawn(async move {
        tokio::time::timeout(Duration::from_secs(30), async move {
            loop {
                let Output::Snapshot(snapshot) = claimant
                    .execute(claimant.prepare(Command::Snapshot))
                    .await
                    .unwrap()
                else {
                    panic!("snapshot expected")
                };
                if let Some(terminal) = snapshot.terminals.first() {
                    let Output::Terminal(info) = claimant
                        .execute(claimant.prepare(Command::ClaimTerminal {
                            terminal: terminal.id,
                            expected_revision: terminal.revision,
                        }))
                        .await
                        .unwrap()
                    else {
                        panic!("claimed terminal expected")
                    };
                    assert_eq!(info.revision, terminal.revision + 1);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Dart terminal creation deadline");
    });
    let invitation = node.link().invite().unwrap();
    let child = super::command("unassigned.dart", directory.path(), invitation.ticket())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let (snapshot, page) = runtime.block_on(async {
        claimed.await.unwrap();
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let session = snapshot.sessions.first().expect("unassigned session");
        let page = client
            .read_conversation(session.id, None, 100)
            .await
            .unwrap()
            .page;
        node.shutdown().await.unwrap();
        (snapshot, page)
    });
    assert!(status.success());
    assert!(snapshot.projects.is_empty());
    assert_eq!(snapshot.sessions.len(), 1);
    let session = &snapshot.sessions[0];
    assert!(session.project.is_none());
    assert!(
        snapshot
            .worktrees
            .iter()
            .any(|tree| tree.id == session.worktree)
    );
    assert_eq!(page.runs.len(), 1);
    assert_eq!(page.runs[0].status, Status::Completed);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}
