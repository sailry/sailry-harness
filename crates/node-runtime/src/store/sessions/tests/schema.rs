use super::*;
use crate::store::database::Database;
use std::path::{Path, PathBuf};

#[test]
fn synchronizes_empty_structure() {
    let (directory, path, node, database) = fixture();
    let request = admit(&database);
    database.connection.execute_batch("ALTER TABLE sessions DROP COLUMN state; DROP TABLE plugin_settings; DROP TABLE model_catalog; DROP TABLE model_catalog_status").unwrap();
    close(database, &path);

    let database = Database::open(&path, node, None).unwrap();
    assert_eq!(database.node, node);
    assert_eq!(
        database
            .connection
            .query_row("SELECT count(state) FROM sessions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    for query in [
        "SELECT body FROM plugin_settings",
        "SELECT body FROM model_catalog",
        "SELECT body FROM model_catalog_status",
    ] {
        assert!(database.connection.prepare(query).is_ok());
    }
    assert_eq!(status(&database.connection, request), "unknown");
    database.close().unwrap();
    let snapshots = snapshots(directory.path());
    assert_eq!(snapshots.len(), 1);
    let snapshot = Connection::open(&snapshots[0]).unwrap();
    assert!(snapshot.prepare("SELECT state FROM sessions").is_err());
    assert!(
        snapshot
            .prepare("SELECT body FROM plugin_settings")
            .is_err()
    );
    assert_eq!(status(&snapshot, request), "admitted");
}

#[test]
fn rejects_missing_state_for_existing_sessions() {
    for state in ["active", "archived", "removed"] {
        let (directory, path, node, database) = fixture();
        let session = session(&database, directory.path(), state);
        let request = admit(&database);
        database
            .connection
            .execute_batch(
                "ALTER TABLE sessions DROP COLUMN state; DROP TABLE model_catalog_status",
            )
            .unwrap();
        close(database, &path);

        let connection = rejected(
            directory.path(),
            &path,
            node,
            request,
            "session lifecycle state is missing",
        );
        assert_eq!(
            connection
                .query_row("SELECT id FROM sessions", [], |row| row.get::<_, String>(0))
                .unwrap(),
            session.to_string()
        );
        assert!(connection.prepare("SELECT state FROM sessions").is_err());
        assert!(
            connection
                .prepare("SELECT body FROM model_catalog_status")
                .is_err()
        );
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
fn rejects_required_columns() {
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
        let request = admit(&database);
        database.connection.execute(remove, []).unwrap();
        close(database, &path);
        let connection = rejected(directory.path(), &path, node, request, column);
        assert!(connection.prepare(query).is_err());
    }
}

fn fixture() -> (tempfile::TempDir, PathBuf, NodeId, Database) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("node.sqlite3");
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
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::High,
                mode: WorkMode::Code,
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
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("schema-backup-")
        })
        .collect()
}
