use super::{fixture::Fixture, *};
use crate::agent::canonical::{self, BlockKind};

async fn claim(fixture: &mut Fixture, message: Input) -> Invocation {
    let turn = fixture.queued().await;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::EditQueuedTurn {
            turn,
            expected_revision: 1,
            message,
        },
    )
    .await;
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn captures_effective_input_without_history() {
    let mut fixture = Fixture::new().await;
    let message: Input = "Edited input 中文 🙂\n".into();
    let invocation = claim(&mut fixture, message.clone()).await;
    assert_eq!(invocation.message, message);
    let original: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT r.body FROM requests r JOIN turns t ON t.request=r.id WHERE t.id=?1")
        .await
        .unwrap()
        .query_row([invocation.turn.id.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    let original: Request = serde_json::from_slice(&original).unwrap();
    let Command::QueueTurn {
        message: queued, ..
    } = original.command
    else {
        panic!("queue request expected")
    };
    assert_eq!(queued.text, "History fixture");
    assert!(
        queue::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    runs::finish(
        &mut fixture.database,
        invocation.turn.id,
        Status::Failed,
        Some(Fault::new(
            ErrorCode::NotConfigured,
            "model is not configured",
        )),
        &fixture.events,
    )
    .await
    .unwrap();
    let page = fixture.read(None, 100).await.page;
    assert!(page.entries.is_empty());
    assert_eq!(page.runs[0].input.as_ref().unwrap().message, message);
    assert!(
        runs::get(&fixture.database.connection, invocation.turn.id)
            .await
            .unwrap()
            .input
            .is_none()
    );
    assert!(
        activity::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .run
            .unwrap()
            .input
            .is_none()
    );
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(fixture.read(None, 100).await.page, page);
    let stored: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT input FROM agent_runs WHERE turn=?1")
        .await
        .unwrap()
        .query_row([invocation.turn.id.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert_eq!(serde_json::from_slice::<Input>(&stored).unwrap(), message);
    assert!(
        serde_json::from_slice::<serde_json::Value>(&stored)
            .unwrap()
            .get("provider")
            .is_none()
    );
}

#[tokio::test]
async fn canonical_user_replaces_capture() {
    let mut fixture = Fixture::new().await;
    let invocation = claim(&mut fixture, "Captured input".into()).await;
    assert!(fixture.read(None, 100).await.page.runs[0].input.is_some());
    let mut event = fixture::message(
        "user",
        canonical::Role::User,
        vec![BlockKind::Text {
            text: "Canonical input".into(),
        }],
    );
    event.turn = invocation.turn.id;
    history::append(
        &mut fixture.database,
        fixture.session,
        invocation.turn.id,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
    let page = fixture.read(None, 100).await.page;
    assert!(page.runs[0].input.is_none());
    assert_eq!(page.entries[0].author, "user");
    assert_eq!(
        page.entries[0].parts,
        [Part::Text("Canonical input".into())]
    );
    let capture: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT input FROM agent_runs WHERE turn=?1")
        .await
        .unwrap()
        .query_row([invocation.turn.id.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Input>(&capture).unwrap(),
        invocation.message
    );
}

#[tokio::test]
async fn preserves_records_when_adding_capture_column() {
    let mut fixture = Fixture::new().await;
    let invocation = claim(&mut fixture, "Old sent input".into()).await;
    runs::finish(
        &mut fixture.database,
        invocation.turn.id,
        Status::Failed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    let path = fixture
        ._root
        .path()
        .canonicalize()
        .unwrap()
        .join("node.turso");
    let node = fixture.database.node;
    let body: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT body FROM agent_runs")
        .await
        .unwrap()
        .query_row(())
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    fixture
        .database
        .connection
        .execute_batch("ALTER TABLE agent_runs DROP COLUMN input")
        .await
        .unwrap();
    fixture.database.close().await.unwrap();
    let database = Database::open(&path, node, None).await.unwrap();
    assert_eq!(
        database
            .connection
            .prepare("SELECT body FROM agent_runs")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<Vec<u8>>(0))
            .unwrap(),
        body
    );
    assert!(
        database
            .connection
            .prepare("SELECT input FROM agent_runs")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<Option<Vec<u8>>>(0))
            .unwrap()
            .is_none()
    );
    let backups = std::fs::read_dir(fixture._root.path().join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    let path = backups[0].join("node.turso");
    let handle = crate::store::database::Handle::new(
        path.clone(),
        crate::store::database::engine(&path).await.unwrap(),
    );
    let backup = handle.lease(true).await.unwrap();
    assert!(
        backup
            .prepare("SELECT input FROM agent_runs")
            .await
            .is_err()
    );
    assert_eq!(
        backup
            .prepare("SELECT body FROM agent_runs")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<Vec<u8>>(0))
            .unwrap(),
        body
    );
    assert_eq!(
        backup
            .prepare("SELECT COUNT(*) FROM requests")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        database
            .connection
            .prepare("SELECT COUNT(*) FROM requests")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap()
    );
    drop(backup);
    drop(handle.pause().await.unwrap());
    database.close().await.unwrap();
}

#[tokio::test]
async fn rejects_invalid_capture_before_upgrade() {
    let mut fixture = Fixture::new().await;
    let invocation = claim(&mut fixture, "Preserve capture".into()).await;
    runs::finish(
        &mut fixture.database,
        invocation.turn.id,
        Status::Failed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    let path = fixture
        ._root
        .path()
        .canonicalize()
        .unwrap()
        .join("node.turso");
    let node = fixture.database.node;
    fixture
        .database
        .connection
        .execute(
            "UPDATE agent_runs SET input=?1",
            [b"invalid input".as_slice()],
        )
        .await
        .unwrap();
    fixture
        .database
        .connection
        .execute_batch("DROP TABLE code_index_settings")
        .await
        .unwrap();
    fixture.database.close().await.unwrap();
    let before = std::fs::read(&path).unwrap();
    let error = Database::open(&path, node, None).await.err().unwrap();
    assert!(error.to_string().contains("agent_runs.input"));
    assert!(!error.to_string().contains("invalid input"));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!fixture._root.path().join("backups").exists());
}
