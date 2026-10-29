//! Current native schemas synchronize in place; old SQLite paths remain untouched.
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};
use turso::{Connection, Database, Value};

#[path = "schema/legacy.rs"]
mod legacy;

#[tokio::test]
async fn preserves_current_resources_on_both_paths() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        let identity = node.id();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let source = directory.path().join("source");
        std::fs::create_dir(&source).unwrap();
        execute(
            &client,
            Command::RegisterProject {
                name: "Schema fixture".into(),
                path: source.to_str().unwrap().into(),
            },
        )
        .await;
        let Output::Plugins(plugins) = execute(&client, Command::ListPlugins).await else {
            panic!("plugin list expected");
        };
        let goals = plugins
            .iter()
            .find(|plugin| plugin.name == "goals")
            .unwrap();
        let Output::Plugin(goals) = execute(
            &client,
            Command::SetPluginEnabled {
                name: "goals".into(),
                expected_revision: goals.revision,
                enabled: false,
            },
        )
        .await
        else {
            panic!("plugin information expected");
        };
        let Output::Snapshot(before) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        drop(client);
        node.shutdown().await.unwrap();
        let files = entries(&profile.join("storage"));

        let node = Node::start(&profile).await.unwrap();
        assert_eq!(node.id(), identity);
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let Output::Snapshot(after) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        assert_eq!(after.projects, before.projects);
        assert_eq!(after.worktrees, before.worktrees);
        let Output::Plugin(restored) = execute(
            &client,
            Command::ReadPlugin {
                name: "goals".into(),
            },
        )
        .await
        else {
            panic!("plugin information expected");
        };
        assert_eq!(restored, goals);
        drop(client);
        node.shutdown().await.unwrap();
        let (_database, connection) = open(&profile.join("storage/node.turso"), true).await;
        assert_eq!(
            connection
                .prepare("PRAGMA user_version")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap()
                .get::<i64>(0)
                .unwrap(),
            1
        );
        drop(connection);
        drop(_database);
        assert_eq!(entries(&profile.join("storage")), files);
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn adds_missing_structures_on_both_paths() {
    for remote in [false, true] {
        for sql in [
            "DROP TABLE model_catalog_status",
            "ALTER TABLE plugin_values DROP COLUMN index_data",
            "DROP INDEX agent_events_turn",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let profile = directory.path().join("node");
            Node::start(&profile)
                .await
                .unwrap()
                .shutdown()
                .await
                .unwrap();
            let path = profile.join("storage/node.turso");
            let before;
            {
                let (_database, connection) = open(&path, false).await;
                connection.execute_batch(sql).await.unwrap();
                before = rows(&connection, "SELECT name,type,sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name,type").await;
                checkpoint(&connection).await;
            }
            let node = Node::start(&profile).await.unwrap();
            let controller =
                Link::controller(directory.path().join("controller"), NetworkScope::default())
                    .await
                    .unwrap();
            let address = controller
                .handle()
                .pair(node.link().invite().unwrap().ticket())
                .await
                .unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(address)
            } else {
                node.local()
            });
            assert!(matches!(
                execute(&client, Command::Snapshot).await,
                Output::Snapshot(_)
            ));
            drop(client);
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
            let saved = backups(&profile);
            assert_eq!(saved.len(), 1, "{sql}");
            {
                let (_database, connection) = open(&saved[0], true).await;
                assert_eq!(rows(&connection, "SELECT name,type,sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name,type").await, before);
            }
            Node::start(&profile)
                .await
                .unwrap()
                .shutdown()
                .await
                .unwrap();
            assert_eq!(backups(&profile).len(), 1, "{sql}");
        }
    }
}

#[tokio::test]
async fn rejects_changes_without_rewriting_profile() {
    for (sql, detail) in [
        (
            "ALTER TABLE agent_runs ADD COLUMN unused BLOB",
            "agent_runs.unused",
        ),
        (
            "ALTER TABLE plugin_values DROP COLUMN index_data; ALTER TABLE plugin_values ADD COLUMN index_data TEXT",
            "plugin_values.index_data",
        ),
        (
            "CREATE TABLE foreign_records(value BLOB)",
            "foreign_records",
        ),
        ("PRAGMA user_version=2", "user_version"),
    ] {
        assert_rejected(sql, detail).await;
    }
}

#[tokio::test]
async fn rejects_invalid_archives_during_updates() {
    assert_rejected("DROP INDEX events_kind; INSERT INTO counters(namespace,value) VALUES('events',1) ON CONFLICT(namespace) DO UPDATE SET value=value+1; INSERT INTO events(cursor,body) SELECT value,CAST('{\"kind\":\"foreign_fixture\",\"data\":null}' AS BLOB) FROM counters WHERE namespace='events'", "events.body").await;
}

#[tokio::test]
async fn preserves_old_sqlite_files() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    Node::start(&profile)
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
    let storage = profile.join("storage");
    for name in [
        "node.turso",
        "node.db-log",
        "node.turso-wal",
        "node.turso-tshm",
        "knowledge.turso",
        "knowledge.db-log",
        "knowledge.turso-wal",
        "knowledge.turso-tshm",
    ] {
        match std::fs::remove_file(storage.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("isolated native fixture cleanup failed: {error}"),
        }
    }
    let old_files = [
        "node.sqlite3",
        "node.sqlite3-wal",
        "node.sqlite3-shm",
        "knowledge.db",
        "knowledge.db-wal",
        "knowledge.db-shm",
    ];
    for name in old_files {
        std::fs::write(storage.join(name), b"untouched old format").unwrap();
    }
    assert!(!storage.join("node.turso").exists());
    assert!(!storage.join("knowledge.turso").exists());
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
        panic!("snapshot expected");
    };
    assert!(snapshot.sessions.is_empty());
    drop(client);
    node.shutdown().await.unwrap();
    assert!(storage.join("node.turso").is_file());
    assert!(storage.join("knowledge.turso").is_file());
    for name in old_files {
        assert_eq!(
            std::fs::read(storage.join(name)).unwrap(),
            b"untouched old format"
        );
    }
}

async fn assert_rejected(sql: &str, detail: &str) {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    Node::start(&profile)
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
    let path = profile.join("storage/node.turso");
    {
        let (_database, connection) = open(&path, false).await;
        connection.execute_batch(sql).await.unwrap();
        checkpoint(&connection).await;
    }
    let before = std::fs::read(&path).unwrap();
    let error = Node::start(&profile)
        .await
        .err()
        .expect("incompatible profile must be rejected")
        .to_string();
    assert!(error.contains(detail), "{error}");
    assert!(error.contains("existing data was not changed"), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

async fn open(path: &Path, read_only: bool) -> (Database, Connection) {
    let database = turso::Builder::new_local(path.to_str().unwrap())
        .experimental_index_method(true)
        .read_only(read_only)
        .build()
        .await
        .unwrap();
    let connection = database.connect().unwrap();
    connection
        .pragma_update("synchronous", "FULL")
        .await
        .unwrap();
    connection
        .pragma_update("foreign_keys", "ON")
        .await
        .unwrap();
    (database, connection)
}

async fn checkpoint(connection: &Connection) {
    connection
        .prepare("PRAGMA wal_checkpoint(TRUNCATE)")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap();
}

async fn rows(connection: &Connection, sql: &str) -> Vec<Vec<Value>> {
    let mut rows = connection.query(sql, ()).await.unwrap();
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        result.push(
            (0..row.column_count())
                .map(|column| row.get_value(column).unwrap())
                .collect(),
        );
    }
    result
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

fn backups(profile: &Path) -> Vec<PathBuf> {
    let directory = profile.join("storage/backups");
    if !directory.exists() {
        return Vec::new();
    }
    let mut files = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("node.turso"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn entries(directory: &Path) -> Vec<OsString> {
    let mut entries = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    entries
}
