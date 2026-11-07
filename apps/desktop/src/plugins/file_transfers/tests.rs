use super::*;
use crate::plugins::fixture::Fixture;
use core::prelude::v1::test;

fn access(fixture: &Fixture) -> Access {
    fixture.package();
    let path = fixture.directory.path().join("project/package/plugin.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["extensions"]["dev.sailry.platform"]["actions"] =
        json!(["files.read", "files.write", "git.read"]);
    std::fs::write(path, manifest.to_string()).unwrap();
    let package = fixture.install(0);
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    Access {
        client: fixture.binding.client.clone(),
        runtime: fixture.runtime.clone(),
        context: plugin::Context {
            invocation: None,
            turn: None,
            surface: plugin::desktop::Surface::Workspace,
            package: package.summary.reference(),
            worktree: fixture.binding.worktree,
            session: None,
        },
        label: "Captured fixture".into(),
    }
}

async fn run(
    target: Access,
    source: worker::Source,
    path: &str,
    prepared: Option<worker::Prepared>,
    cancel: CancellationToken,
) -> worker::Completion {
    let (updates, _changes) = tokio::sync::watch::channel(Progress::Preparing);
    let retained = Arc::new(std::sync::Mutex::new(prepared.clone()));
    let result = worker::run(
        target,
        source,
        path.into(),
        None,
        worker::Recovery {
            prepared,
            retained: retained.clone(),
        },
        cancel,
        updates,
    )
    .await;
    if result.prepared.is_some() {
        assert!(retained.lock().unwrap().is_some());
    }
    result
}

#[test]
fn captured_publication() {
    for (scenario, check) in [
        (
            "reveals_only_local_registered_paths",
            (|_scenario: &str| {
                for remote in [false, true] {
                    let fixture = Fixture::new(remote);
                    let access = access(&fixture);
                    let requests = fixture.transport.requests.lock().unwrap().len();
                    let result = fixture.runtime.block_on(system::prepare(
                        system::Action::Reveal,
                        access.clone(),
                        "notes.txt".into(),
                        !remote,
                        fixture.directory.path().join("temporary"),
                        CancellationToken::new(),
                    ));
                    if remote {
                        assert_eq!(
                            result.err().unwrap().code,
                            ErrorCode::InvalidRequest,
                            "reveals only local registered paths"
                        );
                        assert_eq!(
                            fixture.transport.requests.lock().unwrap().len(),
                            requests,
                            "reveals only local registered paths"
                        );
                        assert!(
                            !fixture.directory.path().join("temporary").exists(),
                            "reveals only local registered paths"
                        );
                    } else {
                        let opened = result.unwrap();
                        assert!(opened.copy.is_none(), "reveals only local registered paths");
                        assert_eq!(
                            opened.path,
                            fixture
                                .directory
                                .path()
                                .join("project/notes.txt")
                                .canonicalize()
                                .unwrap(),
                            "reveals only local registered paths"
                        );
                        let outside = fixture.runtime.block_on(system::prepare(
                            system::Action::Reveal,
                            access,
                            "../outside.txt".into(),
                            true,
                            fixture.directory.path().join("temporary"),
                            CancellationToken::new(),
                        ));
                        assert!(outside.is_err(), "reveals only local registered paths");
                    }
                    fixture.close();
                }
            }) as fn(&str),
        ),
        (
            "retains_captured_locations_and_publication_receipts",
            (|_scenario: &str| {
                let fixture = Fixture::new(true);
                let source = access(&fixture);
                let root = fixture.directory.path().join("other");
                std::fs::create_dir(&root).unwrap();
                let Output::Project(project) = fixture.execute(Command::RegisterProject {
                    name: "Destination".into(),
                    path: root.to_str().unwrap().into(),
                }) else {
                    panic!("project expected")
                };
                let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
                    panic!("snapshot expected")
                };
                let tree = snapshot
                    .worktrees
                    .iter()
                    .find(|tree| tree.project == Some(project.id))
                    .unwrap()
                    .id;
                let mut target = source.clone();
                target.context.worktree = Some(tree);
                let entry = worker::Source::Entry {
                    access: Box::new(source.clone()),
                    path: "notes.txt".into(),
                    kind: sailry_protocol::EntryKind::File,
                    cut: false,
                };
                let completion = fixture.runtime.block_on(run(
                    target.clone(),
                    entry.clone(),
                    "copy.txt",
                    None,
                    CancellationToken::new(),
                ));
                assert_eq!(
                    completion.result,
                    Ok(()),
                    "retains captured locations and publication receipts"
                );
                assert!(
                    completion.published,
                    "retains captured locations and publication receipts"
                );
                assert_eq!(
                    std::fs::read(root.join("copy.txt")).unwrap(),
                    std::fs::read(fixture.directory.path().join("project/notes.txt")).unwrap(),
                    "retains captured locations and publication receipts"
                );
                std::fs::write(root.join("copy.txt"), "External destination edit").unwrap();
                let completed = fixture.runtime.block_on(run(
                    target.clone(),
                    entry,
                    "copy.txt",
                    completion.prepared,
                    CancellationToken::new(),
                ));
                assert_eq!(
                    completed.result,
                    Ok(()),
                    "retains captured locations and publication receipts"
                );
                assert_eq!(
                    std::fs::read_to_string(root.join("copy.txt")).unwrap(),
                    "External destination edit",
                    "retains captured locations and publication receipts"
                );
                std::fs::write(
                    fixture.directory.path().join("project/move.txt"),
                    "Move once",
                )
                .unwrap();
                let entry = worker::Source::Entry {
                    access: Box::new(source),
                    path: "move.txt".into(),
                    kind: sailry_protocol::EntryKind::File,
                    cut: true,
                };
                let completion = fixture.runtime.block_on(run(
                    target,
                    entry,
                    "moved.txt",
                    None,
                    CancellationToken::new(),
                ));
                assert_eq!(
                    completion.result,
                    Ok(()),
                    "retains captured locations and publication receipts"
                );
                assert_eq!(
                    std::fs::read_to_string(root.join("moved.txt")).unwrap(),
                    "Move once",
                    "retains captured locations and publication receipts"
                );
                assert!(
                    !fixture.directory.path().join("project/move.txt").exists(),
                    "retains captured locations and publication receipts"
                );
                fixture.close();
            }),
        ),
        (
            "streams_captured_sources",
            (|_scenario: &str| {
                let source_fixture = Fixture::new(true);
                let target_fixture = Fixture::new(true);
                let source = access(&source_fixture);
                let target = access(&target_fixture);
                let bytes = vec![0xf1; 384 * 1024];
                std::fs::write(
                    source_fixture.directory.path().join("project/source.bin"),
                    &bytes,
                )
                .unwrap();
                let entry = worker::Source::Entry {
                    access: Box::new(source.clone()),
                    path: "source.bin".into(),
                    kind: sailry_protocol::EntryKind::File,
                    cut: false,
                };
                let completion = target_fixture.runtime.block_on(run(
                    target.clone(),
                    entry.clone(),
                    "copy.bin",
                    None,
                    CancellationToken::new(),
                ));
                assert_eq!(completion.result, Ok(()), "streams captured sources");
                assert_eq!(
                    std::fs::read(target_fixture.directory.path().join("project/copy.bin"))
                        .unwrap(),
                    bytes,
                    "streams captured sources"
                );
                std::fs::write(
                    target_fixture.directory.path().join("project/copy.bin"),
                    b"Later",
                )
                .unwrap();
                let replay = target_fixture.runtime.block_on(run(
                    target.clone(),
                    entry,
                    "copy.bin",
                    completion.prepared,
                    CancellationToken::new(),
                ));
                assert_eq!(replay.result, Ok(()), "streams captured sources");
                assert_eq!(
                    std::fs::read(target_fixture.directory.path().join("project/copy.bin"))
                        .unwrap(),
                    b"Later",
                    "streams captured sources"
                );
                let folder = worker::Source::Entry {
                    access: Box::new(source),
                    path: "package".into(),
                    kind: sailry_protocol::EntryKind::Directory,
                    cut: false,
                };
                let failure = target_fixture.runtime.block_on(run(
                    target,
                    folder,
                    "folder",
                    None,
                    CancellationToken::new(),
                ));
                assert_eq!(
                    failure.result.unwrap_err().code,
                    ErrorCode::InvalidRequest,
                    "streams captured sources"
                );
                assert!(
                    !target_fixture
                        .directory
                        .path()
                        .join("project/folder")
                        .exists(),
                    "streams captured sources"
                );
                source_fixture.close();
                target_fixture.close();
            }),
        ),
        (
            "stops_before_admission_when_cancelled_or_disabled",
            (|_scenario: &str| {
                let fixture = Fixture::new(true);
                let target = access(&fixture);
                let entry = worker::Source::Entry {
                    access: Box::new(target.clone()),
                    path: "notes.txt".into(),
                    kind: sailry_protocol::EntryKind::File,
                    cut: false,
                };
                let stop = CancellationToken::new();
                stop.cancel();
                let cancelled = fixture.runtime.block_on(run(
                    target.clone(),
                    entry.clone(),
                    "cancelled.txt",
                    None,
                    stop,
                ));
                assert_eq!(
                    cancelled.result.unwrap_err().code,
                    ErrorCode::Cancelled,
                    "stops before admission when cancelled or disabled"
                );
                assert!(
                    cancelled.prepared.is_none(),
                    "stops before admission when cancelled or disabled"
                );
                fixture.execute(Command::SetPluginEnabled {
                    name: target.context.package.name.clone(),
                    expected_revision: 1,
                    enabled: false,
                });
                let disabled = fixture.runtime.block_on(run(
                    target,
                    entry,
                    "disabled.txt",
                    None,
                    CancellationToken::new(),
                ));
                assert_eq!(
                    disabled.result.unwrap_err().code,
                    ErrorCode::NotConfigured,
                    "stops before admission when cancelled or disabled"
                );
                assert!(
                    disabled.prepared.is_none(),
                    "stops before admission when cancelled or disabled"
                );
                assert!(
                    !fixture
                        .directory
                        .path()
                        .join("project/cancelled.txt")
                        .exists(),
                    "stops before admission when cancelled or disabled"
                );
                assert!(
                    !fixture
                        .directory
                        .path()
                        .join("project/disabled.txt")
                        .exists(),
                    "stops before admission when cancelled or disabled"
                );
                fixture.close();
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
