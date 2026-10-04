use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::conversation::{Part, Status};
use sailry_protocol::{Command, Output};

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn restores_goal_revisions() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("project")).unwrap();
    std::fs::write(
        directory.path().join("project/evidence.txt"),
        "Verified evidence 中文 🙂",
    )
    .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::goal_loop(1));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-goals",
    ));
    runtime.block_on(async {
        let client = Client::new(node.local());
        let Output::Plugin(package) = client
            .execute(client.prepare(Command::ReadPlugin {
                name: "goals".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("package expected")
        };
        client
            .execute(client.prepare(Command::SetPluginEnabled {
                name: "goals".into(),
                expected_revision: package.summary.revision,
                enabled: true,
            }))
            .await
            .unwrap();
    });
    let invitation = node.link().invite().unwrap();
    let child = super::command("goals.dart", directory.path(), invitation.ticket())
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
    assert!(pages[0].approvals.is_empty() && pages[0].questions.is_empty());
    assert_eq!(pages[0].entries.iter().flat_map(|entry| &entry.parts).filter(|part| matches!(part, Part::ToolResult { name, .. } if name == &support::plugin_tool("goals", "get_goal") || name == &support::plugin_tool("goals", "update_goal"))).count(), 4);
    assert_eq!(server.requests.lock().unwrap().len(), 8);
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &sessions)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), 8);
}
