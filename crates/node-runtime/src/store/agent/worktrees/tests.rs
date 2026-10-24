use super::*;
use crate::store::{
    agent::tests::{command, fixture::Fixture},
    commands,
};

async fn register(
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
    crate::store::worktrees::register(&fixture.database.connection, &worktree)
        .await
        .unwrap();
    worktree
}

async fn start(fixture: &mut Fixture) -> TurnId {
    let turn = fixture.queued().await;
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    turn
}

#[tokio::test]
async fn freezes_project_scope() {
    let mut fixture = Fixture::new().await;
    let original = commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let source_directory = tempfile::tempdir().unwrap();
    let source = register(&fixture, original.project, source_directory.path()).await;
    let sibling_directory = tempfile::tempdir().unwrap();
    let sibling = register(&fixture, original.project, sibling_directory.path()).await;
    let foreign_directory = tempfile::tempdir().unwrap();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RegisterProject {
            name: "Other project".into(),
            path: foreign_directory.path().to_str().unwrap().into(),
        },
    )
    .await;
    let unassigned_directory = tempfile::tempdir().unwrap();
    register(&fixture, None, unassigned_directory.path()).await;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::MoveConversation {
            session: original.id,
            worktree: source.id,
            expected_revision: original.revision,
        },
    )
    .await;
    let turn = start(&mut fixture).await;
    // A later session revision cannot redirect an admitted tool's scope.
    let mut current = commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    current.worktree = sibling.id;
    commands::revise_session(&fixture.database.connection, current)
        .await
        .unwrap();
    let entries = list(&fixture.database, turn).await.unwrap();
    assert_eq!(
        entries.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        vec![original.worktree, source.id, sibling.id]
    );
    drop(source_directory);
    assert_eq!(
        list(&fixture.database, turn).await.unwrap_err().code,
        ErrorCode::NotFound
    );
    assert!(sibling_directory.path().is_dir());
}

#[tokio::test]
async fn isolates_unassigned_roots() {
    let mut fixture = Fixture::new().await;
    let source_directory = tempfile::tempdir().unwrap();
    let source = register(&fixture, None, source_directory.path()).await;
    let other_directory = tempfile::tempdir().unwrap();
    register(&fixture, None, other_directory.path()).await;
    let mut session = commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    session.id = SessionId::new();
    session.project = None;
    session.worktree = source.id;
    let transaction = fixture
        .database
        .connection
        .unchecked_transaction()
        .await
        .unwrap();
    crate::store::sessions::insert(&transaction, &session)
        .await
        .unwrap();
    crate::store::media::freeze(&transaction, session.id, Some(fixture.session))
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    fixture.session = session.id;
    let turn = start(&mut fixture).await;
    assert_eq!(list(&fixture.database, turn).await.unwrap(), vec![source]);
}

#[tokio::test]
async fn requires_active_turn() {
    let mut fixture = Fixture::new().await;
    assert_eq!(
        list(&fixture.database, TurnId::new())
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let turn = fixture.queued().await;
    assert_eq!(
        list(&fixture.database, turn).await.unwrap_err().code,
        ErrorCode::Conflict
    );
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(list(&fixture.database, turn).await.unwrap().len(), 1);
    runs::stop(&fixture.database.connection, turn)
        .await
        .unwrap();
    assert_eq!(
        list(&fixture.database, turn).await.unwrap_err().code,
        ErrorCode::Conflict
    );
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Cancelled,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    assert_eq!(
        list(&fixture.database, turn).await.unwrap_err().code,
        ErrorCode::Conflict
    );
}
