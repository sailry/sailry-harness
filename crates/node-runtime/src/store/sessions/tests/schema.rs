use super::*;
use crate::store::database::Database;
use std::path::{Path, PathBuf};

#[tokio::test]
async fn restores_missing_state_for_empty_sessions() {
    let (directory, path, node, database) = fixture().await;
    let request = admit(&database).await;
    database
        .connection
        .execute_batch("ALTER TABLE sessions DROP COLUMN state")
        .await
        .unwrap();
    close(database).await;

    restored(
        directory.path(),
        &path,
        node,
        request,
        "sessions",
        "SELECT state FROM sessions",
        0,
    )
    .await;
}

#[tokio::test]
async fn restores_default_state_for_existing_sessions() {
    for state in ["active", "archived", "removed"] {
        let (directory, path, node, database) = fixture().await;
        let session = session(&database, directory.path(), state).await;
        let revision: i64 = database
            .connection
            .prepare("SELECT revision FROM sessions")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get(0))
            .unwrap();
        let request = admit(&database).await;
        database
            .connection
            .execute_batch("ALTER TABLE sessions DROP COLUMN state")
            .await
            .unwrap();
        close(database).await;

        restored(
            directory.path(),
            &path,
            node,
            request,
            "sessions",
            "SELECT state FROM sessions",
            1,
        )
        .await;
        let (_connection_handle, connection) = open(&path, false).await;
        assert_eq!(
            connection
                .prepare("SELECT id,state,revision FROM sessions")
                .await
                .unwrap()
                .query_row(())
                .await
                .and_then(|row| {
                    Ok((
                        row.get::<String>(0)?,
                        row.get::<String>(1)?,
                        row.get::<i64>(2)?,
                    ))
                })
                .unwrap(),
            (session.to_string(), "active".into(), revision)
        );
        let restored = commands::session(&connection, session).await.unwrap();
        assert_eq!(restored.id, session);
        assert!(!restored.archived);
        assert_eq!(restored.revision, revision as u64);
        let (_backup_handle, backup) = open(&snapshots(directory.path())[0], true).await;
        assert_eq!(
            backup
                .prepare("SELECT id,revision FROM sessions")
                .await
                .unwrap()
                .query_row(())
                .await
                .and_then(|row| { Ok((row.get::<String>(0)?, row.get::<i64>(1)?)) })
                .unwrap(),
            (session.to_string(), revision)
        );
        assert!(backup.prepare("SELECT state FROM sessions").await.is_err());
    }
}

#[tokio::test]
async fn preserves_existing_state() {
    for state in ["active", "archived", "removed"] {
        let (directory, path, node, database) = fixture().await;
        let session = session(&database, directory.path(), state).await;
        let revision: i64 = database
            .connection
            .prepare("SELECT revision FROM sessions")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get(0))
            .unwrap();
        close(database).await;

        let database = Database::open(&path, node, None).await.unwrap();
        let saved: (String, i64) = database
            .connection
            .prepare("SELECT state,revision FROM sessions")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap();
        assert_eq!(saved, (state.into(), revision));
        if state == "removed" {
            assert_eq!(
                commands::session(&database.connection, session)
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
        } else {
            let restored = commands::session(&database.connection, session)
                .await
                .unwrap();
            assert_eq!(restored.id, session);
            assert_eq!(restored.archived, state == "archived");
            assert_eq!(restored.revision, revision as u64);
        }
        database.close().await.unwrap();
        assert!(snapshots(directory.path()).is_empty());
    }
}

#[tokio::test]
async fn restores_empty_required_columns() {
    for (remove, query, table) in [
        (
            "ALTER TABLE turns DROP COLUMN kind",
            "SELECT kind FROM turns",
            "turns",
        ),
        (
            "ALTER TABLE turns DROP COLUMN plugins",
            "SELECT plugins FROM turns",
            "turns",
        ),
        (
            "ALTER TABLE session_revisions DROP COLUMN roles",
            "SELECT roles FROM session_revisions",
            "session_revisions",
        ),
    ] {
        let (directory, path, node, database) = fixture().await;
        let request = admit(&database).await;
        database.connection.execute(remove, ()).await.unwrap();
        close(database).await;
        restored(directory.path(), &path, node, request, table, query, 0).await;
    }
}

#[tokio::test]
async fn rejects_populated_required_columns() {
    for (remove, query, column) in [
        (
            "ALTER TABLE turns DROP COLUMN kind",
            "SELECT kind FROM turns",
            "turns.kind",
        ),
        (
            "ALTER TABLE turns DROP COLUMN plugins",
            "SELECT plugins FROM turns",
            "turns.plugins",
        ),
        (
            "ALTER TABLE session_revisions DROP COLUMN roles",
            "SELECT roles FROM session_revisions",
            "session_revisions.roles",
        ),
    ] {
        let (directory, path, node, database) = fixture().await;
        let session = session(&database, directory.path(), "active").await;
        let request = admit(&database).await;
        let turn = TurnId::new();
        database
            .connection
            .execute("BEGIN CONCURRENT", ())
            .await
            .unwrap();
        let sequence = crate::store::database::counters::next(
            &database.connection,
            crate::store::database::counters::Namespace::Turns,
        )
        .await
        .unwrap();
        database.connection.execute(
            "INSERT INTO turns(rowid,id,kind,plugins,session,revision,request,caller) VALUES(?1,?2,'task',?3,?4,1,?5,?6)",
            params![sequence as i64, turn.to_string(), b"[]".as_slice(), session.to_string(), request.to_string(), &node.0[..]],
        ).await.unwrap();
        database.connection.execute("COMMIT", ()).await.unwrap();
        database.connection.execute(remove, ()).await.unwrap();
        close(database).await;

        let (_handle, connection) = rejected(directory.path(), &path, node, request, column).await;
        assert!(connection.prepare(query).await.is_err());
        let saved: (String, String, i64, String, Vec<u8>) = connection
            .prepare("SELECT id,session,revision,request,caller FROM turns")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap();
        assert_eq!(
            saved,
            (
                turn.to_string(),
                session.to_string(),
                1,
                request.to_string(),
                node.0.to_vec()
            )
        );
        assert_eq!(
            connection
                .prepare("SELECT session,revision FROM session_revisions")
                .await
                .unwrap()
                .query_row(())
                .await
                .and_then(|row| { Ok((row.get::<String>(0)?, row.get::<i64>(1)?)) })
                .unwrap(),
            (session.to_string(), 1)
        );
    }
}

#[tokio::test]
async fn uses_the_durable_creation_watermark() {
    let (directory, _path, _node, database) = fixture().await;
    database
        .connection
        .execute(
            "INSERT INTO counters(namespace,value) VALUES('sessions',41)",
            (),
        )
        .await
        .unwrap();
    let id = session(&database, directory.path(), "active").await;
    let restored = commands::session(&database.connection, id).await.unwrap();
    assert_eq!(restored.creation_order, 42);
    database.close().await.unwrap();
}

async fn fixture() -> (tempfile::TempDir, PathBuf, NodeId, Database) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("storage")
        .join("node.turso");
    std::fs::create_dir(path.parent().unwrap()).unwrap();
    let node = NodeId([31; 32]);
    let database = Database::open(&path, node, None).await.unwrap();
    (directory, path, node, database)
}

async fn session(database: &Database, profile: &Path, state: &str) -> SessionId {
    database
        .connection
        .execute("BEGIN CONCURRENT", ())
        .await
        .unwrap();
    let Output::Session(session) = create(
        &database.connection,
        database.node,
        Some(profile),
        None,
        None,
        Configuration {
            config: SessionConfig {
                features: Default::default(),
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::High,
                mode: WorkMode::Code,
                options: Default::default(),
                permission: Permission::Ask,
                credential: None,
            },
            profile: None,
            roles: Default::default(),
        },
    )
    .await
    .unwrap()
    .0
    else {
        panic!("session expected")
    };
    match state {
        "active" => {}
        "archived" => {
            archive(&database.connection, session.id, session.revision, true)
                .await
                .unwrap();
        }
        "removed" => {
            remove(&database.connection, session.id, session.revision)
                .await
                .unwrap();
        }
        _ => panic!("unsupported fixture lifecycle state"),
    }
    database.connection.execute("COMMIT", ()).await.unwrap();
    session.id
}

async fn admit(database: &Database) -> RequestId {
    let request = Request::new(
        database.node,
        Command::CreateSession {
            project: None,
            worktree: None,
            config: None,
        },
    );
    database
        .connection
        .execute(
            "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
            params![
                &database.node.0[..],
                request.id.to_string(),
                serde_json::to_vec(&request).unwrap()
            ],
        )
        .await
        .unwrap();
    request.id
}

async fn close(database: Database) {
    database.close().await.unwrap();
}

async fn rejected(
    directory: &Path,
    path: &Path,
    node: NodeId,
    request: RequestId,
    detail: &str,
) -> (turso::Database, Connection) {
    let before = std::fs::read(path).unwrap();
    let error = Database::open(path, node, None).await.err().unwrap();
    assert!(error.to_string().contains(detail), "{error}");
    assert!(error.to_string().contains("requires a default"), "{error}");
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert!(snapshots(directory).is_empty());
    let (connection_handle, connection) = open(path, false).await;
    assert_eq!(
        connection
            .prepare("PRAGMA journal_mode")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<String>(0))
            .unwrap(),
        "mvcc"
    );
    assert_eq!(status(&connection, request).await, "admitted");
    (connection_handle, connection)
}

async fn restored(
    directory: &Path,
    path: &Path,
    node: NodeId,
    request: RequestId,
    table: &str,
    query: &str,
    count: i64,
) {
    let (original_handle, original) = open(path, false).await;
    let definitions = objects(&original).await;
    let body: Vec<u8> = original
        .prepare("SELECT body FROM requests WHERE id=?1")
        .await
        .unwrap()
        .query_row([request.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert!(original.prepare(query).await.is_err());
    drop(original);
    drop(original_handle);
    let database = Database::open(path, node, None).await.unwrap();
    assert!(database.connection.prepare(query).await.is_ok());
    assert_eq!(
        database
            .connection
            .prepare(&format!("SELECT count(*) FROM {table}"))
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        count
    );
    assert_eq!(status(&database.connection, request).await, "unknown");
    assert_eq!(
        database
            .connection
            .prepare("SELECT body FROM requests WHERE id=?1")
            .await
            .unwrap()
            .query_row([request.to_string()])
            .await
            .and_then(|row| row.get::<Vec<u8>>(0))
            .unwrap(),
        body
    );
    assert_eq!(
        database
            .connection
            .prepare("PRAGMA user_version")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        1
    );
    database.close().await.unwrap();

    let saved = snapshots(directory);
    assert_eq!(saved.len(), 1);
    let (backup_handle, backup) = open(&saved[0], true).await;
    assert_eq!(objects(&backup).await, definitions);
    assert!(backup.prepare(query).await.is_err());
    assert_eq!(
        backup
            .prepare(&format!("SELECT count(*) FROM {table}"))
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        count
    );
    assert_eq!(status(&backup, request).await, "admitted");
    assert_eq!(
        backup
            .prepare("SELECT body FROM requests WHERE id=?1")
            .await
            .unwrap()
            .query_row([request.to_string()])
            .await
            .and_then(|row| row.get::<Vec<u8>>(0))
            .unwrap(),
        body
    );
    assert_eq!(
        backup
            .prepare("PRAGMA user_version")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        1
    );
    drop(backup);
    drop(backup_handle);
    let bytes = std::fs::read(&saved[0]).unwrap();

    let database = Database::open(path, node, None).await.unwrap();
    assert!(database.connection.prepare(query).await.is_ok());
    assert_eq!(
        database
            .connection
            .prepare(&format!("SELECT count(*) FROM {table}"))
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        count
    );
    assert_eq!(status(&database.connection, request).await, "unknown");
    database.close().await.unwrap();
    assert_eq!(snapshots(directory), saved);
    assert_eq!(std::fs::read(&saved[0]).unwrap(), bytes);
}

async fn objects(connection: &Connection) -> Vec<(String, String, Option<String>)> {
    let mut rows = connection.query("SELECT name,type,sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name,type", ()).await.unwrap();
    let mut objects = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        objects.push((
            row.get(0).unwrap(),
            row.get(1).unwrap(),
            row.get(2).unwrap(),
        ));
    }
    objects
}

async fn status(connection: &Connection, request: RequestId) -> String {
    connection
        .prepare("SELECT status FROM requests WHERE id=?1")
        .await
        .unwrap()
        .query_row([request.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap()
}

fn snapshots(directory: &Path) -> Vec<PathBuf> {
    let directory = directory.join("storage").join("backups");
    if !directory.exists() {
        return Vec::new();
    }
    let mut snapshots = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("node.turso"))
        .collect::<Vec<_>>();
    snapshots.sort();
    snapshots
}

async fn open(path: &Path, read_only: bool) -> (turso::Database, Connection) {
    let database = turso::Builder::new_local(path.to_str().unwrap())
        .experimental_index_method(true)
        .read_only(read_only)
        .build()
        .await
        .unwrap();
    let connection = crate::store::database::connect(&database).await.unwrap();
    (database, connection)
}
