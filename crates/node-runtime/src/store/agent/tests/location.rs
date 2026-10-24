use super::*;
use crate::store::commands;

#[tokio::test]
async fn preserves_admitted_locations() {
    let mut fixture = fixture::Fixture::new().await;
    let turn = fixture.completed(&["Original context"]).await;
    let db = &fixture.database.connection;
    let original = commands::session(db, fixture.session).await.unwrap();
    let worktree = Worktree {
        id: WorktreeId::new(),
        project: original.project,
        path: fixture
            .database
            .worktree_root(original.worktree)
            .await
            .unwrap()
            .to_str()
            .unwrap()
            .into(),
        main: false,
    };
    // Use a distinct existing directory so registration remains unique.
    let directory = tempfile::tempdir().unwrap();
    let worktree = Worktree {
        path: directory.path().to_str().unwrap().into(),
        ..worktree
    };
    super::super::super::worktrees::register(db, &worktree)
        .await
        .unwrap();
    let (Output::Session(moved), _) =
        super::super::super::sessions::location::move_to(db, original.id, worktree.id, 1)
            .await
            .unwrap()
    else {
        panic!("session expected")
    };
    assert_eq!(moved.id, original.id);
    assert_eq!(moved.revision, 2);
    assert_eq!(moved.worktree, worktree.id);
    let frozen: String = db.prepare("SELECT r.worktree FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE t.id=?1").await.unwrap().query_row([turn.to_string()]).await.and_then(|row| row.get(0)).unwrap();
    assert_eq!(frozen, original.worktree.to_string());
    let transaction = db.unchecked_transaction().await.unwrap();
    let (Output::Session(fork), _) = super::super::super::sessions::location::fork_at(
        &transaction,
        original.id,
        original.worktree,
        2,
    )
    .await
    .unwrap() else {
        panic!("fork expected")
    };
    transaction.commit().await.unwrap();
    assert_ne!(fork.id, original.id);
    assert_eq!(fork.worktree, original.worktree);
    assert_eq!(fork.fork.as_ref().unwrap().through, turn);
    assert_eq!(
        runs::visible(db, fork.id, turn).await.unwrap().worktree,
        original.worktree
    );
    assert_eq!(
        commands::session(db, original.id).await.unwrap().worktree,
        worktree.id
    );
    assert_eq!(
        super::super::super::sessions::location::move_to(db, original.id, original.worktree, 1)
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let Output::QueuedTurn(queued) = command(
        &mut fixture.database,
        &fixture.events,
        Command::QueueTurn {
            session: original.id,
            expected_revision: 2,
            message: "Next task".into(),
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    assert_eq!(
        super::super::super::sessions::location::move_to(
            &fixture.database.connection,
            original.id,
            original.worktree,
            2
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::Busy
    );
    runs::start(&fixture.database.connection, queued.id)
        .await
        .unwrap();
    let execution = runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(execution.worktree, worktree.id);
}
