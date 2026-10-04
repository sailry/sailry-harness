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
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("project")).unwrap();
    std::fs::write(
        directory.path().join("project/source.txt"),
        "Original evidence 中文 🙂",
    )
    .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::compaction_sequence(
        false,
        "Context summary fixture: retain user constraints and source.txt evidence 中文 🙂".into(),
        if manual {
            vec![100]
        } else {
            // Cross the 4096-token model's automatic budget after a complete
            // tool exchange, then report the reduced context after summary.
            vec![100, 100, 100, 3500, 100]
        },
    ));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-context",
    ));
    let invitation = node.link().invite().unwrap();
    let child = super::command("compaction.dart", directory.path(), invitation.ticket())
        .env("SAILRY_MANUAL_CONTEXT", if manual { "1" } else { "0" })
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
        if manual { 3 } else { 5 }
    );
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &sessions)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
}
