use super::{configuration, configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{
    conversation::{Page, Part, Status},
    *,
};
use serde_json::json;
use std::time::Duration;

pub(super) async fn submit(client: &Client, session: SessionId, message: &str) -> TurnId {
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::SubmitTurn {
            session,
            expected_revision: 1,
            message: message.into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let Output::Conversation(history) = client
                .execute(client.prepare(Command::ReadConversation {
                    session,
                    before: None,
                    limit: 1,
                }))
                .await
                .unwrap()
            else {
                panic!("history expected")
            };
            if let Some(run) = history.page.runs.iter().find(|run| run.turn == turn.id)
                && !matches!(run.status, Status::Queued | Status::Running)
            {
                assert_eq!(run.status, Status::Completed);
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    turn.id
}

pub(super) fn assert_history(page: &Page, users: usize, answers: usize, tools: usize, model: &str) {
    assert_eq!(
        page.entries
            .iter()
            .filter(|entry| entry.author == "user")
            .count(),
        users
    );
    let parts: Vec<_> = page.entries.iter().flat_map(|entry| &entry.parts).collect();
    assert_eq!(
        parts
            .iter()
            .filter(|part| matches!(part, Part::Text(text) if text == &format!("answer-{model}")))
            .count(),
        answers
    );
    let calls: Vec<_> = parts
        .iter()
        .filter_map(|part| match part {
            Part::ToolCall { id, name, .. } => Some((id, name)),
            _ => None,
        })
        .collect();
    let results: Vec<_> = parts
        .iter()
        .filter_map(|part| match part {
            Part::ToolResult { id, name, .. } => Some((id, name)),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), tools, "all fixture tool calls are retained");
    assert_eq!(
        results, calls,
        "complete native tool exchanges retain identity and order"
    );
    assert!(
        page.entries
            .iter()
            .all(|entry| page.runs.iter().any(|run| run.turn == entry.turn))
    );
}

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn loads_during_streaming() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("project")).unwrap();
    std::fs::write(directory.path().join("project/资料.txt"), "History 中文 🙂").unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::tools(
        (0..51)
            .map(|_| {
                (
                    super::agent_support::plugin_tool("files", "read_file"),
                    json!({"path": "资料.txt"}),
                )
            })
            .collect(),
    ));
    let slow = runtime.block_on(support::Server::start(true));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-history",
    ));
    let client = Client::new(node.local());
    let (first, slow_config) = runtime.block_on(async {
        let first = submit(&client, sessions[0], "History 中文 🙂").await;
        for _ in 0..44 {
            submit(&client, sessions[0], "History 中文 🙂").await;
        }
        submit(&client, sessions[1], "History 中文 🙂").await;
        let selected = configuration::provider(&client, &slow.endpoint, "ffi-history-live").await;
        (first, configuration::config(&selected))
    });
    let model_requests = {
        let requests = server.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| !support::compaction::is_summary(request))
                .count(),
            148
        );
        requests.len()
    };
    let invitation = node.link().invite().unwrap();
    let child = super::command("paging.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .env("SAILRY_FIRST_TURN", first.to_string())
        .env(
            "SAILRY_SLOW_CONFIG",
            serde_json::to_string(&slow_config).unwrap(),
        )
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let pages = runtime.block_on(tools::pages(&client, &sessions));
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert_eq!(pages[0].runs.len(), 46);
    assert_history(&pages[0], 46, 45, 51, "ffi-history");
    assert_eq!(pages[0].runs.last().unwrap().status, Status::Cancelled);
    assert_history(&pages[1], 1, 1, 51, "ffi-history");
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &sessions)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), model_requests);
    assert_eq!(slow.requests.lock().unwrap().len(), 1);
}
