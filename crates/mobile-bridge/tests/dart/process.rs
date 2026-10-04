use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use serde_json::json;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn executes_and_stops() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let text = "完整输出 中文 🙂\n```rust\nlet value = 1;\n```\n".repeat(100);
    let command = format!(
        "printf x >> count.txt\nif [ -e stop-mode ]; then printf '%s' \"$$\" > pid.txt; printf ready > ready.txt; sleep 30; printf unexpected > after.txt; fi\nprintf '%s' '{text}'\nprintf 'failure detail\\377' >&2\nexit 7"
    );
    let server = runtime.block_on(support::Server::tools(vec![
        (
            super::agent_support::plugin_tool("commands", "run_command"),
            json!({"command": command, "cwd": "", "timeout_ms": 60_000}),
        ),
        (
            super::agent_support::plugin_tool("commands", "run_command"),
            json!({"command": "printf 'before timeout\\n'; sleep 30", "timeout_ms": 100}),
        ),
        (
            super::agent_support::plugin_tool("commands", "run_command"),
            json!({"command": "printf never > denied.txt"}),
        ),
    ]));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-command",
    ));
    let invitation = node.link().invite().unwrap();
    let child = super::command("process.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let pages = runtime.block_on(tools::pages(&Client::new(node.local()), &sessions));
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert_eq!(
        std::fs::read_to_string(root.join("count.txt")).unwrap(),
        "xx"
    );
    assert!(!root.join("denied.txt").exists());
    assert!(!root.join("after.txt").exists());
    let pid: u32 = std::fs::read_to_string(root.join("pid.txt"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        !std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .unwrap()
            .status
            .success()
    );
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &sessions)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 5);
    assert!(
        requests
            .iter()
            .all(|request| request["model"] == "ffi-command")
    );
}
