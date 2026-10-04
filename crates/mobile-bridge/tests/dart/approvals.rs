use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use serde_json::json;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn resolves_and_cancels() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let text = "完整的审批正文 🙂\nA literal sample: ```rust\nlet value = 1;\n```\n".repeat(100);
    let server = runtime.block_on(support::Server::tools(vec![
        (
            super::agent_support::plugin_tool("files", "write_file"),
            json!({"path": "approved.txt", "text": text, "expected_revision": null}),
        ),
        (
            super::agent_support::plugin_tool("files", "write_file"),
            json!({"path": "denied.txt", "text": "must not be written", "expected_revision": null}),
        ),
    ]));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-approvals",
    ));
    let invitation = node.link().invite().unwrap();
    let child = super::command("approvals.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let pages = runtime.block_on(tools::pages(&Client::new(node.local()), &sessions));
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert_eq!(
        std::fs::read_to_string(root.join("approved.txt")).unwrap(),
        text
    );
    assert!(!root.join("denied.txt").exists());
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &sessions)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert!(
        requests
            .iter()
            .all(|request| request["model"] == "ffi-approvals")
    );
}
