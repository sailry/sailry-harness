#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn observes_multiple_nodes() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let first = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("first"),
        ))
        .unwrap();
    let second = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("second"),
        ))
        .unwrap();
    let first_project = directory.path().join("project-first");
    let second_project = directory.path().join("project-second");
    std::fs::create_dir(&first_project).unwrap();
    std::fs::create_dir(&second_project).unwrap();
    let first_invitation = first.link().invite().unwrap();
    let second_invitation = second.link().invite().unwrap();
    let child = super::command("overview.dart", directory.path(), first_invitation.ticket())
        .env("SAILRY_SECOND_INVITATION", second_invitation.ticket())
        .env("SAILRY_PROJECT_PATH", first_project)
        .env("SAILRY_SECOND_PROJECT_PATH", second_project)
        .spawn()
        .unwrap();
    let status = super::wait(child);
    runtime.block_on(first.shutdown()).unwrap();
    runtime.block_on(second.shutdown()).unwrap();
    assert!(status.success());
}
