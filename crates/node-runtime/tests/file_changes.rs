use crate::file_fixture::Fixture;
use sailry_client::Client;
use sailry_protocol::*;

fn revision(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

async fn apply(
    client: &Client,
    worktree: WorktreeId,
    changes: Vec<FileChange>,
) -> (Request, Result<Output, Fault>) {
    let request = client.prepare(Command::ApplyChanges { worktree, changes });
    let result = client.execute(request.clone()).await;
    (request, result)
}

#[tokio::test]
async fn applies_validated_sets_in_order() {
    let fixture = Fixture::empty().await;
    for (index, client) in fixture.clients().iter().enumerate() {
        let directory = format!("set-{index}");
        let path = |name: &str| format!("{directory}/{name}");
        let read = |name: &str| std::fs::read_to_string(fixture.root.join(path(name))).unwrap();
        std::fs::create_dir_all(fixture.root.join(&directory)).unwrap();
        std::fs::write(fixture.root.join(path("lib.rs")), "fn old() {}\r\n").unwrap();
        std::fs::write(fixture.root.join(path("README.md")), "# Original\n").unwrap();
        std::fs::write(fixture.root.join(path("old.rs")), "moved\n").unwrap();
        std::fs::write(fixture.root.join(path("dead.rs")), "dead\n").unwrap();
        let (request, result) = apply(
            client,
            fixture.worktree,
            vec![
                FileChange::Create {
                    path: path("nested/new.rs"),
                    text: "pub fn new() {}\n".into(),
                },
                FileChange::Write {
                    path: path("README.md"),
                    text: "# Replaced\n".into(),
                    expected_revision: revision("# Original\n"),
                },
                FileChange::Edit {
                    path: path("lib.rs"),
                    expected_revision: revision("fn old() {}\r\n"),
                    edits: vec![FileEdit {
                        old_text: "old() {}\n".into(),
                        new_text: "renamed() {}\n".into(),
                        replace_all: false,
                    }],
                },
                FileChange::Move {
                    path: path("old.rs"),
                    destination: path("moved.rs"),
                    expected_revision: revision("moved\n"),
                },
                FileChange::Delete {
                    path: path("dead.rs"),
                    expected_revision: revision("dead\n"),
                },
            ],
        )
        .await;
        let output = result.unwrap();
        let Output::FileChanges(changes) = &output else {
            panic!("file changes expected")
        };
        assert_eq!(changes.status, ChangesStatus::Applied, "{changes:?}");
        assert!(
            changes
                .files
                .iter()
                .all(|file| file.status == ChangeStatus::Applied && file.error.is_none())
        );
        assert_eq!(read("nested/new.rs"), "pub fn new() {}\n");
        assert_eq!(read("README.md"), "# Replaced\n");
        assert_eq!(read("lib.rs"), "fn renamed() {}\r\n");
        assert_eq!(read("moved.rs"), "moved\n");
        assert!(!fixture.root.join(path("old.rs")).exists());
        assert!(!fixture.root.join(path("dead.rs")).exists());
        assert_eq!(
            changes.files[2].revision.as_deref(),
            Some(revision("fn renamed() {}\r\n").as_str())
        );
        assert!(changes.files[2].excerpt.is_some());
        // The original request identity replays its recorded outcome without writing again.
        std::fs::write(fixture.root.join(path("README.md")), "External\n").unwrap();
        assert_eq!(client.execute(request).await.unwrap(), output);
        assert_eq!(read("README.md"), "External\n");
    }
    fixture.close().await;
}

#[tokio::test]
async fn rejects_invalid_sets_without_writing() {
    let fixture = Fixture::empty().await;
    for (index, client) in fixture.clients().iter().enumerate() {
        let directory = format!("rejected-{index}");
        let path = |name: &str| format!("{directory}/{name}");
        std::fs::create_dir_all(fixture.root.join(&directory)).unwrap();
        std::fs::write(fixture.root.join(path("keep.txt")), "keep\n").unwrap();
        let (_, result) = apply(
            client,
            fixture.worktree,
            vec![
                FileChange::Create {
                    path: path("new.txt"),
                    text: "new\n".into(),
                },
                FileChange::Edit {
                    path: path("keep.txt"),
                    expected_revision: revision("stale\n"),
                    edits: vec![FileEdit {
                        old_text: "keep".into(),
                        new_text: "changed".into(),
                        replace_all: false,
                    }],
                },
            ],
        )
        .await;
        let Output::FileChanges(changes) = result.unwrap() else {
            panic!("file changes expected")
        };
        assert_eq!(changes.status, ChangesStatus::Rejected);
        assert_eq!(changes.files[0].status, ChangeStatus::NotAttempted);
        assert_eq!(changes.files[1].status, ChangeStatus::Failed);
        assert_eq!(
            changes.files[1].error.as_ref().map(|error| error.code),
            Some(ErrorCode::RevisionConflict)
        );
        assert!(!fixture.root.join(path("new.txt")).exists());
        // Protected paths and empty sets fail the whole command before any write.
        let (_, protected) = apply(
            client,
            fixture.worktree,
            vec![
                FileChange::Create {
                    path: path("allowed.txt"),
                    text: "x".into(),
                },
                FileChange::Create {
                    path: ".git/hooks/pre-commit".into(),
                    text: "x".into(),
                },
            ],
        )
        .await;
        assert_eq!(protected.unwrap_err().code, ErrorCode::PermissionDenied);
        assert!(!fixture.root.join(path("allowed.txt")).exists());
        let (_, empty) = apply(client, fixture.worktree, Vec::new()).await;
        assert_eq!(empty.unwrap_err().code, ErrorCode::InvalidRequest);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(path("keep.txt"))).unwrap(),
            "keep\n"
        );
    }
    fixture.close().await;
}
