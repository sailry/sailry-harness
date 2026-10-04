use super::configuration::{config, provider, support};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output};

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn edits_and_resumes() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::start(false));
    let (node, session) = runtime.block_on(async {
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(node.local());
        let selected = provider(&client, &server.endpoint, "ffi-queue").await;
        let Output::Project(project) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "FFI queue".into(),
                path: directory.path().to_str().unwrap().into(),
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
        (node, session)
    });
    let invitation = node.link().invite().unwrap();
    let child = super::command("queue.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSION", session.id.to_string())
        .env("SAILRY_WORKTREE", session.worktree.to_string())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| request["model"] == "ffi-queue")
    );
    assert!(
        requests[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"].as_str().is_some_and(
                |text| text.contains("Attachment: input.txt\nComplete FFI attachment input")
            ))
    );
}
