use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::conversation::{
    Part, Status,
    question::{Answer, State},
};
use sailry_protocol::{Command, Output};
use serde_json::json;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn preserves_frozen_work_modes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let write = super::agent_support::plugin_tool("files", "write_file");
    let server = runtime.block_on(support::Server::tools(vec![
        (
            "ask_user".into(),
            json!({"prompt":"Review the frozen plan 中文 🙂","input":{"kind":"plan"}}),
        ),
        (
            write.clone(),
            json!({
                "path": "mode.txt", "text": "Coding after planning 中文 🙂", "expected_revision": null
            }),
        ),
    ]));
    let coding = runtime.block_on(support::Server::tools(vec![(
        write.clone(),
        json!({
            "path": "frozen-code.txt", "text": "Frozen coding 中文 🙂", "expected_revision": null
        }),
    )]));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-mode",
    ));
    runtime.block_on(async {
        let client = Client::new(node.local());
        let mut provider =
            super::configuration::provider(&client, &coding.endpoint, "ffi-code").await;
        provider.models[0].tools = true;
        client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider: provider.clone(),
            }))
            .await
            .unwrap();
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.id == sessions[1])
            .unwrap();
        client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config: super::configuration::config(&provider),
            }))
            .await
            .unwrap();
    });
    let invitation = node.link().invite().unwrap();
    let child = super::command("planning.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
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
    assert_eq!(pages[0].questions.len(), 1);
    let State::Answered(Answer::Plan { turn: coding_turn }) = &pages[0].questions[0].state else {
        panic!("accepted plan expected")
    };
    assert_ne!(*coding_turn, pages[0].questions[0].turn);
    assert_eq!(pages[0].approvals.len(), 1);
    assert_eq!(pages[0].approvals[0].turn, *coding_turn);
    assert_eq!(pages[1].runs.len(), 1);
    assert_eq!(pages[1].runs[0].status, Status::Completed);
    assert_eq!(pages[1].approvals.len(), 1);
    for page in &pages {
        assert_eq!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::ToolCall { name, .. } if name == &write))
                .count(),
            1
        );
        assert_eq!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::ToolResult { name, .. } if name == &write))
                .count(),
            1
        );
    }
    assert_eq!(
        std::fs::read_to_string(root.join("mode.txt")).unwrap(),
        "Coding after planning 中文 🙂"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("frozen-code.txt")).unwrap(),
        "Frozen coding 中文 🙂"
    );
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests
            .iter()
            .map(|request| request["messages"]
                .to_string()
                .contains("This is a planning turn"))
            .collect::<Vec<_>>(),
        [true, false, false]
    );
    let coding_requests = coding.requests.lock().unwrap().clone();
    assert_eq!(coding_requests.len(), 2);
    assert!(coding_requests.iter().all(|request| {
        !request["messages"]
            .to_string()
            .contains("This is a planning turn")
    }));
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &sessions)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), requests.len());
    assert_eq!(coding.requests.lock().unwrap().len(), coding_requests.len());
}
