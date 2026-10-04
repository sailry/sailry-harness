use super::{configuration, configuration::support, paging::submit, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::conversation::{Status, search::Query};

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn locates_committed_turns() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("project")).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::start(false));
    let slow = runtime.block_on(support::Server::start(true));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-search",
    ));
    let client = Client::new(node.local());
    let query = Query {
        text: "äbc 中文 🙂".into(),
        case_sensitive: false,
        before: None,
        limit: 7,
    };
    let (found, slow_config) = runtime.block_on(async {
        submit(
            &client,
            sessions[0],
            "Marker ÄBC 中文 🙂 and literal .* [text]",
        )
        .await;
        for index in 0..44 {
            submit(&client, sessions[0], &format!("History {index} 中文 🙂")).await;
        }
        submit(&client, sessions[1], "Other session").await;
        let found = client
            .search_conversation(sessions[0], query.clone())
            .await
            .unwrap();
        let selected = configuration::provider(&client, &slow.endpoint, "ffi-search-live").await;
        (found, configuration::config(&selected))
    });
    let model_requests = {
        let requests = server.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| !support::compaction::is_summary(request))
                .count(),
            46
        );
        requests.len()
    };
    let invitation = node.link().invite().unwrap();
    let child = super::command("search.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .env(
            "SAILRY_SEARCH_MATCH",
            serde_json::to_string(&found.matches[0]).unwrap(),
        )
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
    assert_eq!(pages[0].runs.len(), 47);
    super::paging::assert_history(&pages[0], 47, 46, 0, "ffi-search");
    assert_eq!(pages[0].runs.last().unwrap().status, Status::Cancelled);
    super::paging::assert_history(&pages[1], 1, 1, 0, "ffi-search");
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    let client = Client::new(node.local());
    assert_eq!(runtime.block_on(tools::pages(&client, &sessions)), pages);
    assert_eq!(
        runtime
            .block_on(client.search_conversation(sessions[0], query))
            .unwrap(),
        found
    );
    runtime.block_on(node.shutdown()).unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), model_requests + 1);
    assert_eq!(slow.requests.lock().unwrap().len(), 1);
}
