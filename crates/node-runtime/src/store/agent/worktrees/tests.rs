use super::*;
use crate::store::{
    agent::tests::{command, fixture::Fixture},
    commands,
};

fn register(
    fixture: &Fixture,
    project: Option<ProjectId>,
    directory: &std::path::Path,
) -> Worktree {
    let worktree = Worktree {
        id: WorktreeId::new(),
        project,
        path: directory.canonicalize().unwrap().to_str().unwrap().into(),
        main: false,
    };
    crate::store::worktrees::register(&fixture.database.connection, &worktree).unwrap();
    worktree
}

fn start(fixture: &mut Fixture) -> TurnId {
    let turn = fixture.queued();
    runs::start(&fixture.database.connection, turn).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    turn
}

#[test]
fn freezes_project_scope() {
    let mut fixture = Fixture::new();
    let original = commands::session(&fixture.database.connection, fixture.session).unwrap();
    let source_directory = tempfile::tempdir().unwrap();
    let source = register(&fixture, original.project, source_directory.path());
    let sibling_directory = tempfile::tempdir().unwrap();
    let sibling = register(&fixture, original.project, sibling_directory.path());
    let foreign_directory = tempfile::tempdir().unwrap();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RegisterProject {
            name: "Other project".into(),
            path: foreign_directory.path().to_str().unwrap().into(),
        },
    );
    let unassigned_directory = tempfile::tempdir().unwrap();
    register(&fixture, None, unassigned_directory.path());
    command(
        &mut fixture.database,
        &fixture.events,
        Command::MoveConversation {
            session: original.id,
            worktree: source.id,
            expected_revision: original.revision,
        },
    );
    let turn = start(&mut fixture);
    // A later session revision cannot redirect an admitted tool's scope.
    let mut current = commands::session(&fixture.database.connection, fixture.session).unwrap();
    current.worktree = sibling.id;
    commands::revise_session(&fixture.database.connection, current).unwrap();
    let entries = list(&fixture.database, turn).unwrap();
    assert_eq!(
        entries.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        vec![original.worktree, source.id, sibling.id]
    );
    drop(source_directory);
    assert_eq!(
        list(&fixture.database, turn).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert!(sibling_directory.path().is_dir());
}

#[test]
fn isolates_unassigned_roots() {
    let mut fixture = Fixture::new();
    let source_directory = tempfile::tempdir().unwrap();
    let source = register(&fixture, None, source_directory.path());
    let other_directory = tempfile::tempdir().unwrap();
    register(&fixture, None, other_directory.path());
    let mut session = commands::session(&fixture.database.connection, fixture.session).unwrap();
    session.id = SessionId::new();
    session.project = None;
    session.worktree = source.id;
    crate::store::sessions::insert(&fixture.database.connection, &session).unwrap();
    crate::store::media::freeze(
        &fixture.database.connection,
        session.id,
        Some(fixture.session),
    )
    .unwrap();
    fixture.session = session.id;
    let turn = start(&mut fixture);
    assert_eq!(list(&fixture.database, turn).unwrap(), vec![source]);
}

#[test]
fn requires_active_turn() {
    let mut fixture = Fixture::new();
    assert_eq!(
        list(&fixture.database, TurnId::new()).unwrap_err().code,
        ErrorCode::NotFound
    );
    let turn = fixture.queued();
    assert_eq!(
        list(&fixture.database, turn).unwrap_err().code,
        ErrorCode::Conflict
    );
    runs::start(&fixture.database.connection, turn).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    assert_eq!(list(&fixture.database, turn).unwrap().len(), 1);
    runs::stop(&fixture.database.connection, turn).unwrap();
    assert_eq!(
        list(&fixture.database, turn).unwrap_err().code,
        ErrorCode::Conflict
    );
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Cancelled,
        None,
        &fixture.events,
    )
    .unwrap();
    assert_eq!(
        list(&fixture.database, turn).unwrap_err().code,
        ErrorCode::Conflict
    );
}
