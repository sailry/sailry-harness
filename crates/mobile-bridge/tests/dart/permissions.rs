use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output};
use serde_json::json;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn preserves_frozen_authority() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::tools(vec![
        (super::agent_support::plugin_tool("files", "write_file"), json!({"path": "permission.txt", "text": "自动与手动 中文 🙂", "expected_revision": null})),
        (super::agent_support::plugin_tool("commands", "run_command"), json!({"command": "printf x >> count.txt"})),
    ]));
    let (node, mut sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-permission",
    ));
    let client = Client::new(node.local());
    let Output::Snapshot(snapshot) = runtime
        .block_on(client.execute(client.prepare(Command::Snapshot)))
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let template = &snapshot.sessions[0];
    let Output::Session(session) = runtime
        .block_on(client.execute(client.prepare(Command::CreateSession {
            project: template.project,
            worktree: Some(template.worktree),
            config: Some(template.config.clone()),
        })))
        .unwrap()
    else {
        panic!("session expected")
    };
    sessions.push(session.id);
    let invitation = node.link().invite().unwrap();
    let child = super::command("permissions.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let pages = runtime.block_on(tools::pages(&client, &sessions));
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert_eq!(
        std::fs::read_to_string(root.join("count.txt")).unwrap(),
        "xxx"
    );
    for mode in ["ask", "project", "full"] {
        assert_eq!(
            std::fs::read_to_string(root.join(format!("permission-{mode}.txt"))).unwrap(),
            "自动与手动 中文 🙂"
        );
    }
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &sessions)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 9);
    assert!(
        requests
            .iter()
            .all(|request| request["model"] == "ffi-permission")
    );
}
