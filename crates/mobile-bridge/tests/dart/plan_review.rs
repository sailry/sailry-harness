use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::conversation::{
    Status,
    question::{Answer, State},
};
use serde_json::json;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn accepts_plans() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::tools(vec![
        ("ask_user".into(), json!({"prompt": "Review the plan 中文 🙂", "input": {"kind": "plan"}})),
        (super::agent_support::plugin_tool("files", "write_file"), json!({"path": "review.txt", "text": "Accepted from Dart 中文 🙂", "expected_revision": null})),
    ]));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-plan-review",
    ));
    let invitation = node.link().invite().unwrap();
    let child = super::command("plan_review.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSION", sessions[0].to_string())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let pages = runtime.block_on(tools::pages(&Client::new(node.local()), &sessions));
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert_eq!(pages[0].runs.len(), 2);
    assert!(
        pages[0]
            .runs
            .iter()
            .all(|run| run.status == Status::Completed)
    );
    assert!(matches!(
        pages[0].questions[0].state,
        State::Answered(Answer::Plan { .. })
    ));
    assert_eq!(pages[0].approvals.len(), 1);
    assert_eq!(
        std::fs::read_to_string(root.join("review.txt")).unwrap(),
        "Accepted from Dart 中文 🙂"
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
