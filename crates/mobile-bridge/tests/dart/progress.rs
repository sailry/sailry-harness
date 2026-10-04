use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::conversation::{Part, Status};
use serde_json::json;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn restores_task_progress() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("project")).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::tools(
        ["in_progress", "completed"]
            .into_iter()
            .map(|state| {
                (
                    support::plugin_tool("progress", "update_plan"),
                    json!({"title": "Task 中文 🙂", "steps": [
                        {"description": "Inspect", "state": "completed"},
                        {"description": "Verify", "state": state},
                        {"description": "Publish", "state": "skipped"}
                    ]}),
                )
            })
            .collect(),
    ));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-progress",
    ));
    let invitation = node.link().invite().unwrap();
    let child = super::command("progress.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSION", sessions[0].to_string())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let pages = runtime.block_on(tools::pages(&Client::new(node.local()), &sessions));
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert_eq!(pages[0].runs.len(), 1);
    assert_eq!(pages[0].runs[0].status, Status::Completed);
    assert!(pages[0].approvals.is_empty() && pages[0].questions.is_empty());
    assert_eq!(
        pages[0]
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter(|part| matches!(part, Part::ToolResult { name, .. } if name == &support::plugin_tool("progress", "update_plan")))
            .count(),
        2
    );
    assert_eq!(server.requests.lock().unwrap().len(), 3);
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &sessions)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), 3);
}
