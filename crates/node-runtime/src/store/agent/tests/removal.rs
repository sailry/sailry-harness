use super::*;
use crate::store::{commands, sessions::location, worktrees};
use std::path::Path;

async fn linked(fixture: &fixture::Fixture) -> (Worktree, tempfile::TempDir) {
    let db = &fixture.database.connection;
    let session = commands::session(db, fixture.session).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let worktree = Worktree {
        id: WorktreeId::new(),
        project: session.project,
        path: directory.path().to_str().unwrap().into(),
        main: false,
    };
    worktrees::register(db, &worktree).await.unwrap();
    location::move_to(db, session.id, worktree.id, session.revision)
        .await
        .unwrap();
    (worktree, directory)
}

async fn queue(fixture: &mut fixture::Fixture) -> Fault {
    let revision = commands::session(&fixture.database.connection, fixture.session)
        .await
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
            .await
            .is_none()
    );
    match response.await.unwrap() {
        Err(error) => error,
        Ok(admission) => admission.completion.await.unwrap().unwrap_err(),
    }
}

#[tokio::test]
async fn keeps_idle_history() {
    let mut fixture = fixture::Fixture::new().await;
    fixture.completed(&["Main answer"]).await;
    let (worktree, _directory) = linked(&fixture).await;
    fixture.completed(&["Linked answer"]).await;
    let history = fixture.read(None, 100).await;
    fixture
        .database
        .validate_removal(worktree.id, Path::new(&worktree.path))
        .await
        .unwrap();
    let mut result = Ok(Output::WorktreeRemoved { id: worktree.id });
    let event = worktrees::finish(&fixture.database.connection, &mut result, false)
        .await
        .unwrap();
    assert!(
        matches!(event, Some(sailry_protocol::Event::WorktreeRemoved { id }) if id == worktree.id)
    );
    assert_eq!(fixture.read(None, 100).await, history);
    let db = &fixture.database.connection;
    assert_eq!(
        commands::session(db, fixture.session)
            .await
            .unwrap()
            .worktree,
        worktree.id
    );
    assert!(
        worktrees::list(db)
            .await
            .unwrap()
            .iter()
            .all(|entry| entry.id != worktree.id)
    );
    assert_eq!(
        fixture
            .database
            .worktree_root(worktree.id)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let error = queue(&mut fixture).await;
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(error.message, "worktree was removed");
    assert_eq!(fixture.read(None, 100).await, history);
}

#[tokio::test]
async fn refuses_active_turns() {
    let mut fixture = fixture::Fixture::new().await;
    let (worktree, _directory) = linked(&fixture).await;
    let root = Path::new(&worktree.path).to_owned();
    let turn = fixture.queued().await;
    let busy = async |fixture: &fixture::Fixture| {
        fixture
            .database
            .validate_removal(worktree.id, &root)
            .await
            .unwrap_err()
            .code
    };
    assert_eq!(busy(&fixture).await, ErrorCode::Busy);
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(busy(&fixture).await, ErrorCode::Busy);
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    fixture
        .database
        .validate_removal(worktree.id, &root)
        .await
        .unwrap();
}
