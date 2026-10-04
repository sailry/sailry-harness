use super::*;
use crate::store::commands;

#[test]
fn preserves_admitted_locations() {
    let mut fixture = fixture::Fixture::new();
    let turn = fixture.completed(&["Original context"]);
    let db = &fixture.database.connection;
    let original = commands::session(db, fixture.session).unwrap();
    let worktree = Worktree {
        id: WorktreeId::new(),
        project: original.project,
        path: fixture
            .database
            .worktree_root(original.worktree)
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
    super::super::super::worktrees::register(db, &worktree).unwrap();
    let (Output::Session(moved), _) =
        super::super::super::sessions::location::move_to(db, original.id, worktree.id, 1).unwrap()
    else {
        panic!("session expected")
    };
    assert_eq!(moved.id, original.id);
    assert_eq!(moved.revision, 2);
    assert_eq!(moved.worktree, worktree.id);
    let frozen: String = db.query_row("SELECT r.worktree FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE t.id=?1", [turn.to_string()], |row| row.get(0)).unwrap();
    assert_eq!(frozen, original.worktree.to_string());
    let (Output::Session(fork), _) =
        super::super::super::sessions::location::fork_at(db, original.id, original.worktree, 2)
            .unwrap()
    else {
        panic!("fork expected")
    };
    assert_ne!(fork.id, original.id);
    assert_eq!(fork.worktree, original.worktree);
    assert_eq!(fork.fork.as_ref().unwrap().through, turn);
    assert_eq!(
        runs::visible(db, fork.id, turn).unwrap().worktree,
        original.worktree
    );
    assert_eq!(
        commands::session(db, original.id).unwrap().worktree,
        worktree.id
    );
    assert_eq!(
        super::super::super::sessions::location::move_to(db, original.id, original.worktree, 1)
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
    ) else {
        panic!("turn expected")
    };
    assert_eq!(
        super::super::super::sessions::location::move_to(
            &fixture.database.connection,
            original.id,
            original.worktree,
            2
        )
        .unwrap_err()
        .code,
        ErrorCode::Busy
    );
    runs::start(&fixture.database.connection, queued.id).unwrap();
    let execution = runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    assert_eq!(execution.worktree, worktree.id);
}
