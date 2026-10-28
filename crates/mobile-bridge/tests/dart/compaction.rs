use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::conversation::Part;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn restores_context_summaries() {
    run(false);
}

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn requests_context_compaction() {
    run(true);
}

fn run(manual: bool) {
    let latest =
        "Keep source.txt unchanged. Complete the requested task without modifying unrelated files.";
    let summary =
        "Context summary fixture: retain user constraints and source.txt evidence 中文 🙂";
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("project")).unwrap();
    std::fs::write(
        directory.path().join("project/source.txt"),
        "Original evidence 中文 🙂",
    )
    .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(async {
        if manual {
            support::Server::compaction_sequence(
                false,
                serde_json::json!({
                    "user_intent": [latest],
                    "current_work": summary,
                    "files": [{"path": "source.txt", "summary": "Read without changes"}]
                })
                .to_string(),
                vec![100],
            )
            .await
        } else {
            // The 128000-token window triggers at 102400. The first completed
            // turn crosses it; the summary and subsequent requests report the
            // reduced context. The server returns the SDK's raw-text fallback.
            support::Server::summary_sequence(vec![120_000, 100]).await
        }
    });
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-context",
    ));
    let invitation = node.link().invite().unwrap();
    let child = super::command("compaction.dart", directory.path(), invitation.ticket())
        .env("SAILRY_MANUAL_CONTEXT", if manual { "1" } else { "0" })
        .env("SAILRY_LATEST_INSTRUCTION", latest)
        .env("SAILRY_SESSION", sessions[0].to_string())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let pages = runtime.block_on(tools::pages(&Client::new(node.local()), &sessions));
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert_eq!(
        pages[0]
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter(|part| matches!(part, Part::Compaction(_)))
            .count(),
        1
    );
    assert_eq!(
        server.requests.lock().unwrap().len(),
        if manual { 4 } else { 5 }
    );
    if manual {
        let rendered = pages[0]
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .find_map(|part| match part {
                Part::Compaction(text) => Some(text),
                _ => None,
            })
            .unwrap();
        assert!(rendered.contains(summary));
        let requests = server.requests.lock().unwrap();
        assert!(support::compaction::is_summary(&requests[3]));
        assert!(requests[3]["messages"].to_string().contains(latest));
    }
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &sessions)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
}
