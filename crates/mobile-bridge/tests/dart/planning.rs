use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use serde_json::json;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn preserves_frozen_work_modes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::tools(vec![(
        super::agent_support::plugin_tool("files", "write_file"),
        json!({
            "path": "mode.txt", "text": "Coding after planning 中文 🙂", "expected_revision": null
        }),
    )]));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-mode",
    ));
    let invitation = node.link().invite().unwrap();
    let child = super::command("planning.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let pages = runtime.block_on(tools::pages(&Client::new(node.local()), &sessions));
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert!(pages[0].approvals.is_empty());
    assert_eq!(pages[1].approvals.len(), 1);
    assert_eq!(
        std::fs::read_to_string(root.join("mode.txt")).unwrap(),
        "Coding after planning 中文 🙂"
    );
    let requests = server.requests.lock().unwrap().clone();
    assert!(requests.iter().any(|request| {
        request["messages"]
            .to_string()
            .contains("This is a planning turn")
    }));
    for request in &requests {
        let planning = request["messages"]
            .to_string()
            .contains("This is a planning turn");
        let writes = request["tools"].as_array().unwrap().iter().any(|tool| {
            tool["function"]["name"] == super::agent_support::plugin_tool("files", "write_file")
        });
        assert_eq!(writes, !planning);
    }
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &sessions)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), requests.len());
}
