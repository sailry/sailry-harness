#[path = "../../../node-runtime/tests/ssh/server.rs"]
#[allow(dead_code)]
mod server;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn controls_execution_owned_ssh() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("node"),
        ))
        .unwrap();
    let server = runtime.block_on(server::Server::start(directory.path().into(), 53));
    let bytes: Vec<_> = (0..128 * 1024).map(|index| (index % 251) as u8).collect();
    std::fs::write(directory.path().join("upload.bin"), &bytes).unwrap();
    let invitation = node.link().invite().unwrap();
    let child = super::command("ssh.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SSH_PORT", server.port.to_string())
        .env("SAILRY_SSH_ID", sailry_protocol::SshId::new().to_string())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert_eq!(
        std::fs::read(directory.path().join("effects")).unwrap(),
        b"x"
    );
    assert_eq!(server.commands.lock().unwrap().len(), 4);
    assert_eq!(
        std::fs::read(directory.path().join("remote.bin")).unwrap(),
        bytes
    );
    assert_eq!(
        std::fs::read(directory.path().join("download.bin")).unwrap(),
        bytes
    );
    assert_eq!(
        std::fs::read(directory.path().join("terminal-effects")).unwrap(),
        b"x"
    );
    runtime.block_on(server.close());
}
