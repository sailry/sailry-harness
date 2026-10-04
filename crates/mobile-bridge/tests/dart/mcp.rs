use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{conversation::Status, *};
use serde_json::json;
use std::fs;

use super::mcp_peer as peer;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn controls_host_tools() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("project/package");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("plugin.json"), json!({"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "example", "version": "1.0.0"}).to_string()).unwrap();
    fs::write(package.join("mcp.json"), json!({"$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json", "mcpServers": {"native": peer::config("mcp_peer::stdio_peer", "normal")}}).to_string()).unwrap();
    fs::write(package.join("version.txt"), "Installed version 中文 🙂").unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let content = "完整 MCP 内容🙂".repeat(1500);
    let server = runtime.block_on(support::Server::tools(vec![
        (peer::alias("native", "read"), json!({})),
        (peer::alias("native", "write"), json!({"value": content})),
        (peer::alias("native", "write"), json!({"value": "denied"})),
    ]));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-mcp",
    ));
    let references = runtime.block_on(async {
        let client = Client::new(node.local());
        let Output::Plugins(packages) = execute(&client, Command::ListPlugins).await else {
            panic!("packages expected")
        };
        for package in packages.into_iter().filter(|package| package.enabled) {
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: package.name,
                    expected_revision: package.revision,
                    enabled: false,
                },
            )
            .await;
        }
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let Output::Plugin(installed) = execute(
            &client,
            Command::InstallPlugin {
                worktree: snapshot.sessions[0].worktree,
                path: "package".into(),
                name: "example".into(),
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        assert_eq!(installed.mcp.len(), 1);
        assert!(installed.issues.is_empty());
        let references = vec![installed.summary.reference()];
        references
    });
    fs::write(package.join("version.txt"), "Modified source").unwrap();
    fs::write(package.join("mcp.json"), "Invalid modified source").unwrap();
    let invitation = node.link().invite().unwrap();
    let child = super::command("mcp.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let (snapshot, pages) = runtime.block_on(async {
        let client = Client::new(node.local());
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        (snapshot, tools::pages(&client, &sessions).await)
    });
    let data = node.profile().join("plugins/data/example");
    let pid: u32 = fs::read_to_string(data.join("pid"))
        .unwrap()
        .parse()
        .unwrap();
    let reaped = !std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .unwrap()
        .status
        .success();
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert!(
        reaped,
        "stopped MCP child must be reaped before Node shutdown"
    );
    assert_eq!(pages[0].runs[0].status, Status::Completed);
    assert_eq!(pages[1].runs[0].status, Status::Cancelled);
    assert!(snapshot.turns.iter().all(|turn| turn.plugins == references));
    assert_eq!(fs::read_to_string(data.join("effects")).unwrap(), content);
    assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "xx");
    assert!(!directory.path().join("controller/plugins").exists());
    runtime.block_on(async {
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(node.local());
        assert_eq!(tools::pages(&client, &sessions).await, pages);
        let Output::Snapshot(restored) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert_eq!(restored.sessions, snapshot.sessions);
        assert_eq!(restored.turns, snapshot.turns);
        node.shutdown().await.unwrap();
    });
    assert_eq!(fs::read_to_string(data.join("effects")).unwrap(), content);
    assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "xx");
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 6);
    let results: Vec<serde_json::Value> = requests[3]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| serde_json::from_str(message["content"].as_str().unwrap()).unwrap())
        .collect();
    assert_eq!(results[0]["output"]["version"], "Installed version 中文 🙂");
    assert_eq!(results[1]["output"]["value"], content);
    assert!(results[2].get("error").is_some());
    let calls: Vec<serde_json::Value> = fs::read_to_string(data.join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|request| request["method"] == "tools/call")
        .collect();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls
            .iter()
            .filter(|request| request["params"]["name"] == "write")
            .count(),
        1
    );
    assert!(server.authorization.lock().unwrap().iter().all(|header| header.as_deref() == Some("Bearer isolated-ffi-configuration-credential")));
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}
