#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn transfers_and_reconnects() {
    contract("attachments.dart");
}

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn downloads_message_images() {
    contract("images.dart");
}

fn contract(script: &str) {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("node"),
        ))
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let child = super::command(script, directory.path(), invitation.ticket())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
}
