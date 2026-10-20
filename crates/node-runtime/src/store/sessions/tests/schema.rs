use super::*;
use crate::store::database::Database;
use std::path::{Path, PathBuf};

#[test]
fn restores_missing_state_for_empty_sessions() {
    let (directory, path, node, database) = fixture();
    let request = admit(&database);
    database
        .connection
        .execute_batch("ALTER TABLE sessions DROP COLUMN state")
        .unwrap();
    close(database, &path);

    restored(
        directory.path(),
        &path,
        node,
        request,
        "sessions",
        "SELECT state FROM sessions",
        0,
    );
}

#[test]
fn restores_default_state_for_existing_sessions() {
    for state in ["active", "archived", "removed"] {
        let (directory, path, node, database) = fixture();
        let session = session(&database, directory.path(), state);
        let revision: i64 = database
            .connection
            .query_row("SELECT revision FROM sessions", [], |row| row.get(0))
            .unwrap();
        let request = admit(&database);
        database
            .connection
            .execute_batch("ALTER TABLE sessions DROP COLUMN state")
            .unwrap();
        close(database, &path);

        restored(
            directory.path(),
            &path,
            node,
            request,
            "sessions",
            "SELECT state FROM sessions",
            1,
        );
        let connection = Connection::open(&path).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT id,state,revision FROM sessions", [], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .unwrap(),
            (session.to_string(), "active".into(), revision)
        );
        let restored = commands::session(&connection, session).unwrap();
        assert_eq!(restored.id, session);
        assert!(!restored.archived);
        assert_eq!(restored.revision, revision as u64);
        let backup = Connection::open_with_flags(
            &snapshots(directory.path())[0],
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            backup
                .query_row("SELECT id,revision FROM sessions", [], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .unwrap(),
            (session.to_string(), revision)
        );
        assert!(backup.prepare("SELECT state FROM sessions").is_err());
    }
}

#[test]
fn preserves_existing_state() {
    for state in ["active", "archived", "removed"] {
        let (directory, path, node, database) = fixture();
        let session = session(&database, directory.path(), state);
        let revision: i64 = database
            .connection
            .query_row("SELECT revision FROM sessions", [], |row| row.get(0))
            .unwrap();
        close(database, &path);

        let database = Database::open(&path, node, None).unwrap();
        let saved: (String, i64) = database
            .connection
            .query_row("SELECT state,revision FROM sessions", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(saved, (state.into(), revision));
        if state == "removed" {
            assert_eq!(
                commands::session(&database.connection, session)
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
        } else {
            let restored = commands::session(&database.connection, session).unwrap();
            assert_eq!(restored.id, session);
            assert_eq!(restored.archived, state == "archived");
            assert_eq!(restored.revision, revision as u64);
        }
        database.close().unwrap();
        assert!(snapshots(directory.path()).is_empty());
    }
}

#[test]
fn restores_empty_required_columns() {
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
        let (directory, path, node, database) = fixture();
        let request = admit(&database);
        database.connection.execute(remove, []).unwrap();
        close(database, &path);
        restored(directory.path(), &path, node, request, table, query, 0);
    }
}

#[test]
fn rejects_populated_required_columns() {
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
        let (directory, path, node, database) = fixture();
        let session = session(&database, directory.path(), "active");
        let request = admit(&database);
        let turn = TurnId::new();
        database.connection.execute(
            "INSERT INTO turns(id,kind,plugins,session,revision,request,caller) VALUES(?1,'task',?2,?3,1,?4,?5)",
            params![turn.to_string(), b"[]".as_slice(), session.to_string(), request.to_string(), &node.0[..]],
        ).unwrap();
        database.connection.execute(remove, []).unwrap();
        close(database, &path);

        let connection = rejected(directory.path(), &path, node, request, column);
        assert!(connection.prepare(query).is_err());
        let saved: (String, String, i64, String, Vec<u8>) = connection
            .query_row(
                "SELECT id,session,revision,request,caller FROM turns",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
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
                .query_row(
                    "SELECT session,revision FROM session_revisions",
                    [],
                    |row| { Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)) }
                )
                .unwrap(),
            (session.to_string(), 1)
        );
    }
}

fn fixture() -> (tempfile::TempDir, PathBuf, NodeId, Database) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("storage")
        .join("node.sqlite3");
    std::fs::create_dir(path.parent().unwrap()).unwrap();
    let node = NodeId([31; 32]);
    let database = Database::open(&path, node, None).unwrap();
    (directory, path, node, database)
}

fn session(database: &Database, profile: &Path, state: &str) -> SessionId {
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
    .unwrap()
    .0
    else {
        panic!("session expected")
    };
    match state {
        "active" => {}
        "archived" => {
            archive(&database.connection, session.id, session.revision, true).unwrap();
        }
        "removed" => {
            remove(&database.connection, session.id, session.revision).unwrap();
        }
        _ => panic!("unsupported fixture lifecycle state"),
    }
    session.id
}

fn admit(database: &Database) -> RequestId {
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
        .unwrap();
    request.id
}

fn close(database: Database, path: &Path) {
    database.close().unwrap();
    let connection = Connection::open(path).unwrap();
    connection
        .pragma_update(None, "journal_mode", "DELETE")
        .unwrap();
    connection.close().unwrap();
}

fn rejected(
    directory: &Path,
    path: &Path,
    node: NodeId,
    request: RequestId,
    detail: &str,
) -> Connection {
    let before = std::fs::read(path).unwrap();
    let error = Database::open(path, node, None).err().unwrap();
    assert!(error.to_string().contains(detail), "{error}");
    assert!(error.to_string().contains("requires a default"), "{error}");
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert!(snapshots(directory).is_empty());
    let connection = Connection::open(path).unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
    assert_eq!(status(&connection, request), "admitted");
    connection
}

fn restored(
    directory: &Path,
    path: &Path,
    node: NodeId,
    request: RequestId,
    table: &str,
    query: &str,
    count: i64,
) {
    let original = Connection::open(path).unwrap();
    let definitions = objects(&original);
    let body: Vec<u8> = original
        .query_row(
            "SELECT body FROM requests WHERE id=?1",
            [request.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(original.prepare(query).is_err());
    original.close().unwrap();
    let database = Database::open(path, node, None).unwrap();
    assert!(database.connection.prepare(query).is_ok());
    assert_eq!(
        database
            .connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        count
    );
    assert_eq!(status(&database.connection, request), "unknown");
    assert_eq!(
        database
            .connection
            .query_row(
                "SELECT body FROM requests WHERE id=?1",
                [request.to_string()],
                |row| row.get::<_, Vec<u8>>(0)
            )
            .unwrap(),
        body
    );
    assert_eq!(
        database
            .connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    database.close().unwrap();

    let saved = snapshots(directory);
    assert_eq!(saved.len(), 1);
    let backup =
        Connection::open_with_flags(&saved[0], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(objects(&backup), definitions);
    assert!(backup.prepare(query).is_err());
    assert_eq!(
        backup
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        count
    );
    assert_eq!(status(&backup, request), "admitted");
    assert_eq!(
        backup
            .query_row(
                "SELECT body FROM requests WHERE id=?1",
                [request.to_string()],
                |row| row.get::<_, Vec<u8>>(0)
            )
            .unwrap(),
        body
    );
    assert_eq!(
        backup
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    backup.close().unwrap();
    let bytes = std::fs::read(&saved[0]).unwrap();

    let database = Database::open(path, node, None).unwrap();
    assert!(database.connection.prepare(query).is_ok());
    assert_eq!(
        database
            .connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        count
    );
    assert_eq!(status(&database.connection, request), "unknown");
    database.close().unwrap();
    assert_eq!(snapshots(directory), saved);
    assert_eq!(std::fs::read(&saved[0]).unwrap(), bytes);
}

fn objects(connection: &Connection) -> Vec<(String, String, Option<String>)> {
    connection
        .prepare("SELECT name,type,sql FROM sqlite_master WHERE name NOT GLOB 'sqlite_*' ORDER BY name,type")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn status(connection: &Connection, request: RequestId) -> String {
    connection
        .query_row(
            "SELECT status FROM requests WHERE id=?1",
            [request.to_string()],
            |row| row.get(0),
        )
        .unwrap()
}

fn snapshots(directory: &Path) -> Vec<PathBuf> {
    let directory = directory.join("storage").join("backups");
    if !directory.exists() {
        return Vec::new();
    }
    let mut snapshots = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("node-before-upgrade-")
                && path
                    .extension()
                    .is_some_and(|extension| extension == "sqlite3")
        })
        .collect::<Vec<_>>();
    snapshots.sort();
    snapshots
}
