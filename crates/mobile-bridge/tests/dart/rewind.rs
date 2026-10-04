use super::{configuration, configuration::support, paging::submit, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output};

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn recovers_without_replay() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("资料.txt"), "Canonical content 中文 🙂").unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::tools(vec![(
        super::agent_support::plugin_tool("files", "read_file"),
        serde_json::json!({"path": "资料.txt"}),
    )]));
    let slow = runtime.block_on(support::Server::start(true));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-rewind",
    ));
    let client = Client::new(node.local());
    let slow_config = runtime.block_on(async {
        submit(&client, sessions[0], "Retained first 中文 🙂").await;
        for index in 1..24 {
            submit(&client, sessions[0], &format!("Removed message {index}")).await;
        }
        submit(&client, sessions[1], "Other conversation").await;
        let selected = configuration::provider(&client, &slow.endpoint, "ffi-rewind-live").await;
        configuration::config(&selected)
    });
    assert_eq!(server.requests.lock().unwrap().len(), 27);
    let baseline = runtime.block_on(tools::pages(&client, &sessions));
    std::fs::write(root.join("资料.txt"), "Changed after original execution").unwrap();
    let invitation = node.link().invite().unwrap();
    let child = super::command("rewind.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .env(
            "SAILRY_SLOW_CONFIG",
            serde_json::to_string(&slow_config).unwrap(),
        )
        .spawn()
        .unwrap();
    assert!(super::wait(child).success());
    let Output::Snapshot(snapshot) = runtime
        .block_on(client.execute(client.prepare(Command::Snapshot)))
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    assert_eq!(snapshot.sessions.len(), 3);
    let backup = snapshot
        .sessions
        .iter()
        .find(|session| session.fork.is_some())
        .unwrap();
    let ids = [sessions[0], sessions[1], backup.id];
    let pages = runtime.block_on(tools::pages(&client, &ids));
    assert_eq!(pages[0].revision, 2);
    assert_eq!(pages[0].runs.len(), 2);
    super::paging::assert_history(&pages[0], 2, 2, 1, "ffi-rewind");
    let first = baseline[0].runs[0].turn;
    assert_eq!(
        pages[0]
            .entries
            .iter()
            .filter(|entry| entry.turn == first)
            .collect::<Vec<_>>(),
        baseline[0]
            .entries
            .iter()
            .filter(|entry| entry.turn == first)
            .collect::<Vec<_>>()
    );
    assert_eq!(pages[1].entries, baseline[1].entries);
    assert_eq!(pages[2].runs.len(), 25);
    super::paging::assert_history(&pages[2], 25, 24, 1, "ffi-rewind");
    let inherited: Vec<_> = pages[2]
        .entries
        .iter()
        .filter(|entry| baseline[0].runs.iter().any(|run| run.turn == entry.turn))
        .collect();
    assert_eq!(inherited, baseline[0].entries.iter().collect::<Vec<_>>());
    let body = server.requests.lock().unwrap().last().unwrap().clone();
    let contents: Vec<_> = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|message| message["content"].as_str())
        .collect();
    assert!(contents.contains(&"Retained first 中文 🙂"));
    assert!(contents.contains(&"Mobile continuation 中文 🙂"));
    assert!(
        !contents
            .iter()
            .any(|message| message.starts_with("Removed message"))
    );
    assert!(!contents.contains(&"Active source turn"));
    runtime.block_on(node.shutdown()).unwrap();
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &ids)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), 28);
    assert_eq!(slow.requests.lock().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(root.join("资料.txt")).unwrap(),
        "Changed after original execution"
    );
}
