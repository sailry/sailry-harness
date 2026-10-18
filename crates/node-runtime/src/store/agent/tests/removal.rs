use super::*;
use crate::store::{commands, sessions::location, worktrees};
use std::path::Path;

fn linked(fixture: &fixture::Fixture) -> (Worktree, tempfile::TempDir) {
    let db = &fixture.database.connection;
    let session = commands::session(db, fixture.session).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let worktree = Worktree {
        id: WorktreeId::new(),
        project: session.project,
        path: directory.path().to_str().unwrap().into(),
        main: false,
    };
    worktrees::register(db, &worktree).unwrap();
    location::move_to(db, session.id, worktree.id, session.revision).unwrap();
    (worktree, directory)
}

fn queue(fixture: &mut fixture::Fixture) -> Fault {
    let revision = commands::session(&fixture.database.connection, fixture.session)
        .unwrap()
        .revision;
    let request = Request::new(
        fixture.database.node,
        Command::QueueTurn {
            session: fixture.session,
            expected_revision: revision,
            message: "Continue".into(),
        },
    );
    let (reply, response) = oneshot::channel();
    assert!(
        fixture
            .database
            .dispatch(fixture.database.node, request, reply, &fixture.events)
            .is_none()
    );
    match response.blocking_recv().unwrap() {
        Err(error) => error,
        Ok(admission) => admission.completion.blocking_recv().unwrap().unwrap_err(),
    }
}

#[test]
fn keeps_idle_history() {
    let mut fixture = fixture::Fixture::new();
    fixture.completed(&["Main answer"]);
    let (worktree, _directory) = linked(&fixture);
    fixture.completed(&["Linked answer"]);
    let history = fixture.read(None, 100);
    fixture
        .database
        .validate_removal(worktree.id, Path::new(&worktree.path))
        .unwrap();
    let mut result = Ok(Output::WorktreeRemoved { id: worktree.id });
    let event = worktrees::finish(&fixture.database.connection, &mut result, false).unwrap();
    assert!(
        matches!(event, Some(sailry_protocol::Event::WorktreeRemoved { id }) if id == worktree.id)
    );
    assert_eq!(fixture.read(None, 100), history);
    let db = &fixture.database.connection;
    assert_eq!(
        commands::session(db, fixture.session).unwrap().worktree,
        worktree.id
    );
    assert!(
        worktrees::list(db)
            .unwrap()
            .iter()
            .all(|entry| entry.id != worktree.id)
    );
    assert_eq!(
        fixture
            .database
            .worktree_root(worktree.id)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let error = queue(&mut fixture);
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(error.message, "worktree was removed");
    assert_eq!(fixture.read(None, 100), history);
}

#[test]
fn refuses_active_turns() {
    let mut fixture = fixture::Fixture::new();
    let (worktree, _directory) = linked(&fixture);
    let root = Path::new(&worktree.path).to_owned();
    let turn = fixture.queued();
    let busy = |fixture: &fixture::Fixture| {
        fixture
            .database
            .validate_removal(worktree.id, &root)
            .unwrap_err()
            .code
    };
    assert_eq!(busy(&fixture), ErrorCode::Busy);
    runs::start(&fixture.database.connection, turn).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    assert_eq!(busy(&fixture), ErrorCode::Busy);
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    fixture
        .database
        .validate_removal(worktree.id, &root)
        .unwrap();
}
