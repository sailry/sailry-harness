use super::{fixture::Fixture, *};

fn claim(fixture: &mut Fixture, message: Input) -> Invocation {
    let turn = fixture.queued();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::EditQueuedTurn {
            turn,
            expected_revision: 1,
            message,
        },
    );
    runs::start(&fixture.database.connection, turn).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap()
}

#[test]
fn captures_effective_input_without_history() {
    let mut fixture = Fixture::new();
    let message: Input = "Edited input 中文 🙂\n".into();
    let invocation = claim(&mut fixture, message.clone());
    assert_eq!(invocation.message, message);
    let original: Vec<u8> = fixture
        .database
        .connection
        .query_row(
            "SELECT r.body FROM requests r JOIN turns t ON t.request=r.id WHERE t.id=?1",
            [invocation.turn.id.to_string()],
            |row| row.get(0),
        )
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
    .unwrap();
    let page = fixture.read(None, 100).page;
    assert!(page.entries.is_empty());
    assert_eq!(page.runs[0].input.as_ref().unwrap().message, message);
    assert!(
        runs::get(&fixture.database.connection, invocation.turn.id)
            .unwrap()
            .input
            .is_none()
    );
    assert!(
        activity::read(&fixture.database.connection, fixture.session)
            .unwrap()
            .run
            .unwrap()
            .input
            .is_none()
    );
    history::rebuild(&mut fixture.database.connection).unwrap();
    assert_eq!(fixture.read(None, 100).page, page);
    let stored: Vec<u8> = fixture
        .database
        .connection
        .query_row(
            "SELECT input FROM agent_runs WHERE turn=?1",
            [invocation.turn.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(serde_json::from_slice::<Input>(&stored).unwrap(), message);
    assert!(
        serde_json::from_slice::<serde_json::Value>(&stored)
            .unwrap()
            .get("provider")
            .is_none()
    );
}

#[test]
fn canonical_user_replaces_capture() {
    let mut fixture = Fixture::new();
    let invocation = claim(&mut fixture, "Captured input".into());
    assert!(fixture.read(None, 100).page.runs[0].input.is_some());
    let mut event = AdkEvent::new("canonical-user");
    event.author = "user".into();
    event.set_content(adk_core::Content::new("user").with_text("Canonical input"));
    history::append(
        &mut fixture.database,
        fixture.session,
        invocation.turn.id,
        event,
        &fixture.events,
    )
    .unwrap();
    let page = fixture.read(None, 100).page;
    assert!(page.runs[0].input.is_none());
    assert_eq!(page.entries[0].author, "user");
    assert_eq!(
        page.entries[0].parts,
        [Part::Text("Canonical input".into())]
    );
    let capture: Vec<u8> = fixture
        .database
        .connection
        .query_row(
            "SELECT input FROM agent_runs WHERE turn=?1",
            [invocation.turn.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Input>(&capture).unwrap(),
        invocation.message
    );
}

#[test]
fn preserves_records_when_adding_capture_column() {
    let mut fixture = Fixture::new();
    let invocation = claim(&mut fixture, "Old sent input".into());
    runs::finish(
        &mut fixture.database,
        invocation.turn.id,
        Status::Failed,
        None,
        &fixture.events,
    )
    .unwrap();
    let path = fixture
        ._root
        .path()
        .canonicalize()
        .unwrap()
        .join("node.sqlite3");
    let node = fixture.database.node;
    let body: Vec<u8> = fixture
        .database
        .connection
        .query_row("SELECT body FROM agent_runs", [], |row| row.get(0))
        .unwrap();
    fixture
        .database
        .connection
        .execute_batch("ALTER TABLE agent_runs DROP COLUMN input")
        .unwrap();
    fixture.database.close().unwrap();
    let database = Database::open(&path, node, None).unwrap();
    assert_eq!(
        database
            .connection
            .query_row("SELECT body FROM agent_runs", [], |row| row
                .get::<_, Vec<u8>>(0))
            .unwrap(),
        body
    );
    assert!(
        database
            .connection
            .query_row("SELECT input FROM agent_runs", [], |row| row
                .get::<_, Option<Vec<u8>>>(0))
            .unwrap()
            .is_none()
    );
    let backups = std::fs::read_dir(fixture._root.path().join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    let backup =
        Connection::open_with_flags(&backups[0], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    assert!(backup.prepare("SELECT input FROM agent_runs").is_err());
    assert_eq!(
        backup
            .query_row("SELECT body FROM agent_runs", [], |row| row
                .get::<_, Vec<u8>>(0))
            .unwrap(),
        body
    );
    assert_eq!(
        backup
            .query_row("SELECT COUNT(*) FROM requests", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        database
            .connection
            .query_row("SELECT COUNT(*) FROM requests", [], |row| row
                .get::<_, i64>(0))
            .unwrap()
    );
    database.close().unwrap();
}

#[test]
fn rejects_invalid_capture_before_upgrade() {
    let mut fixture = Fixture::new();
    let invocation = claim(&mut fixture, "Preserve capture".into());
    runs::finish(
        &mut fixture.database,
        invocation.turn.id,
        Status::Failed,
        None,
        &fixture.events,
    )
    .unwrap();
    let path = fixture
        ._root
        .path()
        .canonicalize()
        .unwrap()
        .join("node.sqlite3");
    let node = fixture.database.node;
    fixture
        .database
        .connection
        .execute(
            "UPDATE agent_runs SET input=?1",
            [b"invalid input".as_slice()],
        )
        .unwrap();
    fixture
        .database
        .connection
        .execute_batch("DROP TABLE code_settings")
        .unwrap();
    fixture.database.close().unwrap();
    let before = std::fs::read(&path).unwrap();
    let error = Database::open(&path, node, None).err().unwrap();
    assert!(error.to_string().contains("agent_runs.input"));
    assert!(!error.to_string().contains("invalid input"));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!fixture._root.path().join("backups").exists());
}
