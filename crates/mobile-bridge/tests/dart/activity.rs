use super::{configuration::support, tools};
use serde_json::json;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn shares_read_state() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut nodes = Vec::new();
    let mut servers = Vec::new();
    let mut sessions = Vec::new();
    for index in 0..2 {
        let root = directory.path().join(index.to_string());
        std::fs::create_dir_all(root.join("project")).unwrap();
        let server = runtime.block_on(support::Server::tools(vec![
            (
                super::agent_support::plugin_tool("files", "write_file"),
                json!({"path":"activity.txt","text":"approved activity","expected_revision":null}),
            ),
            (
                "ask_user".into(),
                json!({"prompt":"Continue","input":{"kind":"text","multiline":false,"max_bytes":128}}),
            ),
        ]));
        let (node, created) =
            runtime.block_on(tools::sessions(&root, &server.endpoint, "ffi-activity"));
        nodes.push(node);
        sessions.push(created[0]);
        servers.push(server);
    }
    let invitations: Vec<_> = nodes
        .iter()
        .map(|node| node.link().invite().unwrap())
        .collect();
    let child = super::command("activity.dart", directory.path(), invitations[0].ticket())
        .env("SAILRY_SECOND_INVITATION", invitations[1].ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    for node in nodes {
        runtime.block_on(node.shutdown()).unwrap();
    }
    assert!(status.success());
    for index in 0..2 {
        assert_eq!(
            std::fs::read_to_string(
                directory
                    .path()
                    .join(format!("{index}/project/activity.txt"))
            )
            .unwrap(),
            "approved activity"
        );
    }
}
