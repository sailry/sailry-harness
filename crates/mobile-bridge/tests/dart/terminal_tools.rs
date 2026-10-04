use std::os::unix::fs::PermissionsExt;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn launches_execution_owned_cli() {
    let directory = tempfile::tempdir().unwrap();
    let programs = directory.path().join("programs");
    std::fs::create_dir(&programs).unwrap();
    let program = programs.join("codex");
    std::fs::write(
        &program,
        concat!(
            "#!/bin/sh\n",
            "printf 'started\\n' >> cli-starts.txt\n",
            "printf 'FFI tool ready\\n'\n",
            "while IFS= read -r line; do printf 'FFI tool result: %s\\n' \"$line\"; done\n",
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("node"),
        ))
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let child = super::command("terminal_tools.dart", directory.path(), invitation.ticket())
        .env("SAILRY_TOOL_PATH", programs)
        .spawn()
        .unwrap();
    let status = super::wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("cli-starts.txt")).unwrap(),
        "started\n"
    );
}
