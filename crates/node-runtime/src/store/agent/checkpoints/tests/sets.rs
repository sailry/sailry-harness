use super::super::changes;
use super::*;
use sailry_protocol::{ChangeKind, ChangeStatus, ChangedFile, FileChange, FileChanges, FileEdit};

async fn request(fixture: &mut Fixture, turn: TurnId) -> Request {
    let worktree = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap()
        .worktree;
    mutation(
        fixture,
        turn,
        Command::ApplyChanges {
            worktree,
            changes: vec![
                FileChange::Create {
                    path: "new.txt".into(),
                    text: "created".into(),
                },
                FileChange::Move {
                    path: "old.txt".into(),
                    destination: "moved.txt".into(),
                    expected_revision: version("old").revision,
                },
                FileChange::Write {
                    path: "write.txt".into(),
                    text: "after write".into(),
                    expected_revision: version("before write").revision,
                },
                FileChange::Edit {
                    path: "edit.txt".into(),
                    expected_revision: version("before edit").revision,
                    edits: vec![FileEdit {
                        old_text: "before".into(),
                        new_text: "after".into(),
                        replace_all: false,
                    }],
                },
            ],
        },
    )
    .await
}

fn originals() -> Vec<Option<String>> {
    vec![
        None,
        None,
        Some("before write".into()),
        Some("before edit".into()),
    ]
}

async fn count(fixture: &Fixture, table: &str) -> i64 {
    fixture
        .database
        .connection
        .prepare(format!("SELECT count(*) FROM {table}"))
        .await
        .unwrap()
        .query_row(())
        .await
        .and_then(|row| row.get::<i64>(0))
        .unwrap()
}

#[tokio::test]
async fn saves_one_file_per_content_entry() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let request = request(&mut fixture, turn).await;
    for _ in 0..2 {
        changes::capture(&mut fixture.database, turn, &request, &originals())
            .await
            .unwrap();
    }
    assert_eq!(count(&fixture, "file_checkpoints").await, 1);
    assert_eq!(count(&fixture, "file_checkpoint_entries").await, 2);
    let page = list(
        &fixture.database.connection,
        fixture.session,
        turn,
        None,
        20,
    )
    .await
    .unwrap();
    // Newest first: the last written file of the set leads the page.
    assert_eq!(
        page.files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["edit.txt", "write.txt", "new.txt"]
    );
    assert!(page.next.is_none());
    let first = list(&fixture.database.connection, fixture.session, turn, None, 2)
        .await
        .unwrap();
    assert_eq!(first.files, page.files[..2]);
    let rest = list(
        &fixture.database.connection,
        fixture.session,
        turn,
        first.next,
        2,
    )
    .await
    .unwrap();
    assert_eq!(rest.files, page.files[2..]);
    assert!(rest.next.is_none());
    for (file, before, after) in [
        (&page.files[0], Some("before edit"), "after edit"),
        (&page.files[1], Some("before write"), "after write"),
        (&page.files[2], None, "created"),
    ] {
        let content = read(&fixture.database.connection, fixture.session, file.id)
            .await
            .unwrap();
        assert_eq!(content.before.as_deref(), before);
        assert_eq!(content.after, after);
        assert_eq!(content.file.outcome, RequestOutcome::NotAdmitted);
    }
    let call: String = fixture
        .database
        .connection
        .prepare("SELECT call FROM agent_approvals WHERE request=?1")
        .await
        .unwrap()
        .query_row([request.id.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    // A single file cannot replace the whole set's declared result display.
    assert!(
        saved(&fixture.database.connection, turn, &call)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn rejects_mismatched_originals() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let request = request(&mut fixture, turn).await;
    let mut stale = originals();
    stale[3] = Some("changed edit".into());
    assert_eq!(
        changes::capture(&mut fixture.database, turn, &request, &stale)
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let mut moved = originals();
    moved[1] = Some("old".into());
    assert_eq!(
        changes::capture(&mut fixture.database, turn, &request, &moved)
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        changes::capture(&mut fixture.database, turn, &request, &originals()[..3])
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(count(&fixture, "file_checkpoints").await, 0);
    changes::capture(&mut fixture.database, turn, &request, &originals())
        .await
        .unwrap();
    let mut altered = request.clone();
    let Command::ApplyChanges {
        changes: entries, ..
    } = &mut altered.command
    else {
        unreachable!()
    };
    entries[2] = FileChange::Write {
        path: "write.txt".into(),
        text: "different".into(),
        expected_revision: version("before write").revision,
    };
    assert_eq!(
        changes::capture(&mut fixture.database, turn, &altered, &originals())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn confirms_each_file_from_the_shared_receipt() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let request = request(&mut fixture, turn).await;
    changes::capture(&mut fixture.database, turn, &request, &originals())
        .await
        .unwrap();
    let (reply, _) = oneshot::channel();
    let caller = fixture.database.node;
    assert!(matches!(
        fixture
            .database
            .dispatch(caller, request.clone(), reply, &fixture.events)
            .await,
        Some(crate::store::external::Work::Mutation(_))
    ));
    let file = |path: &str, kind, status, text: Option<&str>| ChangedFile {
        path: path.into(),
        destination: None,
        kind,
        status,
        revision: text.map(|text| version(text).revision),
        size: text.map(|text| text.len() as u64),
        excerpt: None,
        error: None,
    };
    let outcome = Ok(Output::FileChanges(FileChanges::new(vec![
        file(
            "new.txt",
            ChangeKind::Added,
            ChangeStatus::Applied,
            Some("created"),
        ),
        file("old.txt", ChangeKind::Renamed, ChangeStatus::Applied, None),
        file(
            "write.txt",
            ChangeKind::Modified,
            ChangeStatus::Unknown,
            None,
        ),
        file(
            "edit.txt",
            ChangeKind::Modified,
            ChangeStatus::NotAttempted,
            None,
        ),
    ])));
    fixture
        .database
        .finish_external(caller, &request, outcome, &fixture.events)
        .await
        .unwrap();
    let page = list(
        &fixture.database.connection,
        fixture.session,
        turn,
        None,
        20,
    )
    .await
    .unwrap();
    assert_eq!(
        page.files
            .iter()
            .map(|file| (file.path.as_str(), file.receipt()))
            .collect::<Vec<_>>(),
        [
            ("edit.txt", checkpoint::Receipt::Absent),
            ("write.txt", checkpoint::Receipt::Uncertain),
            ("new.txt", checkpoint::Receipt::Written),
        ]
    );
}
