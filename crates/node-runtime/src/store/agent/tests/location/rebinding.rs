use super::*;
use crate::agent::canonical::{Configuration, Context, Kind, Record};
use crate::store::agent::tests::fixture::Fixture;

async fn execute(fixture: &mut Fixture, command: Command) -> sailry_link::Response {
    let (reply, response) = oneshot::channel();
    assert!(
        fixture
            .database
            .dispatch(
                fixture.database.node,
                Request::new(fixture.database.node, command),
                reply,
                &fixture.events,
            )
            .await
            .is_none()
    );
    response.await.unwrap().unwrap().completion.await.unwrap()
}

fn update(project: &Project, path: &std::path::Path) -> Command {
    Command::UpdateProject {
        expected: project.clone(),
        name: project.name.clone(),
        path: path.to_str().unwrap().into(),
        appearance: project.appearance.clone(),
    }
}

async fn events(fixture: &Fixture, turn: TurnId) -> Vec<Vec<u8>> {
    let mut statement = fixture
        .database
        .connection
        .prepare("SELECT body FROM agent_events WHERE turn=?1 ORDER BY sequence")
        .await
        .unwrap();
    let mut rows = statement.query([turn.to_string()]).await.unwrap();
    let mut bodies = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        bodies.push(row.get(0).unwrap());
    }
    bodies
}

#[tokio::test]
async fn preserves_captures_and_future_root() {
    let mut fixture = Fixture::new().await;
    let original = commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let project = crate::store::projects::list(&fixture.database.connection)
        .await
        .unwrap()
        .remove(0);
    let old_root = fixture
        .database
        .worktree_root(original.worktree)
        .await
        .unwrap();
    let mut context = Record::new(
        TurnId::new(),
        Kind::Configuration(Configuration {
            context: Context {
                captured: true,
                environment: Some(format!("Captured directory: {}", old_root.display())),
                ..Default::default()
            },
        }),
    );
    context.provenance.author = "system".into();
    let turn = fixture
        .append([
            context,
            fixture::message(
                "assistant",
                Role::Assistant,
                vec![BlockKind::Text {
                    text: old_root.display().to_string(),
                }],
            ),
        ])
        .await;
    let capture = crate::store::captures::read(&fixture.database.connection, original.id, turn)
        .await
        .unwrap();
    let retained = events(&fixture, turn).await;
    let retained_session = commands::session(&fixture.database.connection, original.id)
        .await
        .unwrap();
    let captured_path = serde_json::to_string(old_root.to_str().unwrap()).unwrap();
    assert!(
        retained
            .iter()
            .any(|body| String::from_utf8_lossy(body).contains(&captured_path))
    );

    let directory = tempfile::tempdir().unwrap();
    let new_root = directory.path().canonicalize().unwrap();
    std::fs::write(new_root.join("marker.txt"), "new checkout").unwrap();
    let Output::Project(updated) = execute(&mut fixture, update(&project, &new_root))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    assert_eq!(updated.id, project.id);
    assert_eq!(updated.path, new_root.to_str().unwrap());
    assert_eq!(
        commands::session(&fixture.database.connection, original.id)
            .await
            .unwrap(),
        retained_session
    );
    assert_eq!(
        fixture
            .database
            .worktree_root(original.worktree)
            .await
            .unwrap(),
        new_root
    );
    assert_eq!(
        crate::store::captures::read(&fixture.database.connection, original.id, turn)
            .await
            .unwrap(),
        capture
    );
    assert_eq!(events(&fixture, turn).await, retained);

    let next = fixture.queued().await;
    runs::start(&fixture.database.connection, next)
        .await
        .unwrap();
    let invocation = runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invocation.worktree, original.worktree);
    assert_eq!(invocation.project, original.project);
    let root = fixture
        .database
        .worktree_root(invocation.worktree)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("marker.txt")).unwrap(),
        "new checkout"
    );
    runs::finish(
        &mut fixture.database,
        next,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    assert_eq!(events(&fixture, turn).await, retained);
    assert_eq!(
        crate::store::captures::read(&fixture.database.connection, original.id, turn)
            .await
            .unwrap(),
        capture
    );
    assert!(old_root.is_dir());
}

#[tokio::test]
async fn drains_admission_budget() {
    let mut fixture = Fixture::new().await;
    let project = crate::store::projects::list(&fixture.database.connection)
        .await
        .unwrap()
        .remove(0);
    let directory = tempfile::tempdir().unwrap();
    let new_root = directory.path().canonicalize().unwrap();
    let held = fixture
        .database
        .writes
        .clone()
        .acquire_owned()
        .await
        .unwrap();
    let mut pending = Box::pin(execute(&mut fixture, update(&project, &new_root)));
    let waiting = tokio::time::timeout(std::time::Duration::from_millis(20), &mut pending).await;
    let blocked = waiting.is_err();
    drop(held);
    let result = match waiting {
        Ok(result) => result,
        Err(_) => tokio::time::timeout(std::time::Duration::from_secs(5), pending)
            .await
            .unwrap(),
    };
    assert!(blocked);
    assert!(matches!(result.unwrap(), Output::Project(_)));
}

#[tokio::test]
async fn rejects_active_turns() {
    let mut fixture = Fixture::new().await;
    let project = crate::store::projects::list(&fixture.database.connection)
        .await
        .unwrap()
        .remove(0);
    let directory = tempfile::tempdir().unwrap();
    let new_root = directory.path().canonicalize().unwrap();
    let turn = fixture.queued().await;
    let capture = crate::store::captures::read(&fixture.database.connection, fixture.session, turn)
        .await
        .unwrap();
    assert_eq!(
        execute(&mut fixture, update(&project, &new_root))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        execute(&mut fixture, update(&project, &new_root))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    runs::stop(&fixture.database.connection, turn)
        .await
        .unwrap();
    assert_eq!(
        runs::get(&fixture.database.connection, turn)
            .await
            .unwrap()
            .status,
        Status::Stopping
    );
    assert_eq!(
        execute(&mut fixture, update(&project, &new_root))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    assert_eq!(
        crate::store::captures::read(&fixture.database.connection, fixture.session, turn)
            .await
            .unwrap(),
        capture
    );
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Interrupted,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    assert!(matches!(
        execute(&mut fixture, update(&project, &new_root))
            .await
            .unwrap(),
        Output::Project(_)
    ));
}
