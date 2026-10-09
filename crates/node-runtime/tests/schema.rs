use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{conversation::*, *};
use serde_json::json;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

#[expect(
    dead_code,
    reason = "The shared model fixture includes response modes used by other integration test binaries"
)]
mod agent_support;

const ADDED_TABLES: [&str; 5] = [
    "agent_approval_rules",
    "code_settings",
    "code_startups",
    "hook_settings",
    "hook_executions",
];

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
        let database = rusqlite::Connection::open_with_flags(
            profile.join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            database
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            1
        );
        database.close().unwrap();
        drop(client);
        node.shutdown().await.unwrap();
        assert_eq!(entries(&profile.join("storage")), files);
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn adds_missing_structures() {
    for (sql, detail) in [
        ("DROP TABLE model_catalog_status", "model_catalog_status"),
        (
            "ALTER TABLE plugin_values DROP COLUMN index_data",
            "plugin_values.index_data",
        ),
        ("DROP INDEX agent_events_turn", "agent_events_turn"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        node.shutdown().await.unwrap();
        let path = profile.join("storage/node.sqlite3");
        let database = rusqlite::Connection::open(&path).unwrap();
        database.execute_batch(sql).unwrap();
        database
            .pragma_update(None, "journal_mode", "DELETE")
            .unwrap();
        database.close().unwrap();

        let objects = rows(
            &open(&path),
            "SELECT name,type,sql FROM sqlite_master WHERE name NOT GLOB 'sqlite_*' ORDER BY name,type",
        );
        let node = Node::start(&profile).await.unwrap();
        let saved = backups(&profile);
        assert_eq!(saved.len(), 1, "{detail}");
        let backup = open(&saved[0]);
        assert_eq!(
            rows(
                &backup,
                "SELECT name,type,sql FROM sqlite_master WHERE name NOT GLOB 'sqlite_*' ORDER BY name,type"
            ),
            objects,
        );
        drop(backup);
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
        node.shutdown().await.unwrap();
        assert_eq!(backups(&profile).len(), 1, "{detail}");
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
            "ALTER TABLE plugin_values DROP COLUMN index_data; ALTER TABLE plugin_values ADD COLUMN index_data BLOB DEFAULT X'00'",
            "plugin_values.index_data",
        ),
        (
            "CREATE TABLE foreign_records(value BLOB)",
            "foreign_records",
        ),
        ("PRAGMA user_version=2", "user_version"),
        (
            "INSERT INTO events(body) VALUES(CAST('{\"kind\":\"foreign_fixture\",\"data\":null}' AS BLOB))",
            "events.body",
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        node.shutdown().await.unwrap();
        let path = profile.join("storage/node.sqlite3");
        let database = rusqlite::Connection::open(&path).unwrap();
        database.execute_batch(sql).unwrap();
        database
            .pragma_update(None, "journal_mode", "DELETE")
            .unwrap();
        database.close().unwrap();
        let before = std::fs::read(&path).unwrap();
        let files = entries(&profile.join("storage"));
        let error = Node::start(&profile)
            .await
            .err()
            .expect("incompatible profile must be rejected");
        let message = error.to_string();
        assert!(
            message.contains("incompatible development database")
                || message.contains("invalid stored JSON"),
            "{message}"
        );
        assert!(message.contains(detail), "{message}");
        assert!(
            message.contains("existing data was not changed"),
            "{message}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(entries(&profile.join("storage")), files);
    }
}

#[tokio::test]
async fn upgrades_resources_on_both_paths() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let source = directory.path().join("project");
        std::fs::create_dir(&source).unwrap();
        let server = agent_support::Server::tools(vec![(
            agent_support::plugin_tool("files", "write_file"),
            json!({"path":"history.txt","text":"Preserved history 中文 🙂","expected_revision":null}),
        )]).await;
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
        let Output::Plugins(plugins) = execute(&client, Command::ListPlugins).await else {
            panic!("plugins expected")
        };
        for package in plugins
            .into_iter()
            .filter(|package| package.enabled && package.name != "files")
        {
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: package.name,
                    expected_revision: package.revision,
                    enabled: false,
                },
            )
            .await;
        }
        let Output::Provider(provider) = execute(
            &client,
            Command::SaveProvider {
                provider: Provider {
                    options: None,
                    oauth: None,
                    id: ProviderId::new(),
                    revision: 0,
                    name: "Schema fixture".into(),
                    api: ModelApi::ChatCompletions,
                    authentication: Authentication::ApiKey,
                    endpoint: server.endpoint.clone(),
                    enabled: true,
                    credential: None,
                    default_model: "schema-fixture".into(),
                    models: vec![Model {
                        id: "schema-fixture".into(),
                        display_name: None,
                        context: 4096,
                        output: 128,
                        vision: false,
                        tools: true,
                        reasoning: false,
                        web_search: false,
                        generates: vec![],
                        efforts: vec![],
                        capabilities: None,
                        custom_efforts: false,
                        default_effort: Effort::Default,
                    }],
                },
                expected_revision: 0,
                secret: Some(Secret::new("schema-fixture-credential".into())),
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let config = SessionConfig {
            assistant: None,
            resource: None,
            provider: provider.id,
            model: "schema-fixture".into(),
            effort: Effort::Default,
            mode: WorkMode::Code,
            permission: Permission::Full,
            credential: provider.credential.clone(),
        };
        execute(
            &client,
            Command::SetDefaults {
                expected_revision: 0,
                config: config.clone(),
            },
        )
        .await;
        let Output::Project(project) = execute(
            &client,
            Command::RegisterProject {
                name: "Preserved project".into(),
                path: source.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let Output::Session(session) = execute(
            &client,
            Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(config),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let settings = execute(
            &client,
            Command::SaveTerminalSettings(terminal::Settings {
                revision: 0,
                shell: String::new(),
                environment: BTreeMap::from([("FIXTURE_VALUE".into(), "preserved".into())]),
            }),
        )
        .await;
        let Output::QueuedTurn(turn) = execute(
            &client,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Write the history fixture".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        let history = finished(&client, session.id, turn.id).await;
        assert_eq!(history.runs[0].status, Status::Completed);
        assert_eq!(history.approvals.len(), 1);
        assert_eq!(history.approvals[0].source, ApprovalSource::Full);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        let Output::Snapshot(before) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        drop(client);
        node.shutdown().await.unwrap();
        let path = profile.join("storage/node.sqlite3");
        let database = rusqlite::Connection::open(&path).unwrap();
        let providers = rows(&database, "SELECT id,body FROM providers ORDER BY id");
        let events = rows(
            &database,
            "SELECT session,id,turn,body FROM agent_events ORDER BY sequence",
        );
        for table in ADDED_TABLES {
            database
                .execute_batch(&format!("DROP TABLE {table}"))
                .unwrap();
        }
        database.execute_batch("ALTER TABLE agent_approvals DROP COLUMN binding; ALTER TABLE agent_runs DROP COLUMN hooks").unwrap();
        database
            .pragma_update(None, "journal_mode", "DELETE")
            .unwrap();
        let objects = rows(
            &database,
            "SELECT name,type,sql FROM sqlite_master WHERE name NOT GLOB 'sqlite_*' ORDER BY name,type",
        );
        database.close().unwrap();
        let files = profile_files(&profile);
        let storage = entries(&profile.join("storage"));
        assert!(backups(&profile).is_empty());

        let node = Node::start(&profile).await.unwrap();
        assert_eq!(node.id(), identity);
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let Output::Snapshot(after) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert_eq!(after.projects, before.projects);
        assert_eq!(after.worktrees, before.worktrees);
        assert_eq!(after.defaults, before.defaults);
        assert_eq!(after.sessions, before.sessions);
        assert_eq!(after.providers, before.providers);
        assert_eq!(after.plugins, before.plugins);
        assert_eq!(
            execute(&client, Command::ReadTerminalSettings).await,
            settings
        );
        assert_eq!(read_history(&client, session.id).await, history);
        let database = open(&path);
        assert!(
            rows(&database, "SELECT id,body FROM providers ORDER BY id") == providers,
            "protected provider rows changed"
        );
        assert_eq!(
            rows(
                &database,
                "SELECT session,id,turn,body FROM agent_events ORDER BY sequence"
            ),
            events
        );
        assert_eq!(
            database
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            1
        );
        for table in ADDED_TABLES {
            assert_eq!(
                database
                    .query_row(
                        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                        [table],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            rows(&database, "SELECT binding FROM agent_approvals"),
            vec![vec![rusqlite::types::Value::Blob(b"null".to_vec())]]
        );
        let hooks: Vec<u8> = database
            .query_row("SELECT hooks FROM agent_runs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<hooks::Settings>(&hooks).unwrap(),
            hooks::Settings::default()
        );
        drop(database);
        let saved = backups(&profile);
        assert_eq!(saved.len(), 1);
        let name = saved[0].file_name().unwrap().to_str().unwrap();
        let nonce = name
            .strip_prefix("node-before-upgrade-")
            .unwrap()
            .strip_suffix(".sqlite3")
            .unwrap();
        assert!(!uuid::Uuid::parse_str(nonce).unwrap().is_nil());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(profile.join("storage/backups"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(&saved[0]).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let backup = open(&saved[0]);
        assert_eq!(
            rows(
                &backup,
                "SELECT name,type,sql FROM sqlite_master WHERE name NOT GLOB 'sqlite_*' ORDER BY name,type"
            ),
            objects
        );
        assert!(
            rows(&backup, "SELECT id,body FROM providers ORDER BY id") == providers,
            "backup lost protected provider rows"
        );
        assert_eq!(
            rows(
                &backup,
                "SELECT session,id,turn,body FROM agent_events ORDER BY sequence"
            ),
            events
        );
        let upgraded = open(&path);
        for (table, order) in [
            ("projects", "id"),
            ("worktrees", "id"),
            ("defaults", "singleton"),
            ("terminal_settings", "singleton"),
            ("sessions", "id"),
            ("session_revisions", "session,revision"),
        ] {
            let sql = format!("SELECT * FROM {table} ORDER BY {order}");
            assert!(
                rows(&backup, &sql) == rows(&upgraded, &sql),
                "upgrade changed {table} records"
            );
        }
        drop(upgraded);
        assert_eq!(
            backup
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            1
        );
        drop(backup);
        let snapshot = std::fs::read(&saved[0]).unwrap();
        drop(client);
        node.shutdown().await.unwrap();
        assert!(
            profile_files(&profile) == files,
            "upgrade changed unrelated profile files"
        );
        let mut expected = storage;
        expected.push(OsString::from("backups"));
        expected.sort();
        assert_eq!(entries(&profile.join("storage")), expected);

        let node = Node::start(&profile).await.unwrap();
        assert_eq!(node.id(), identity);
        assert_eq!(backups(&profile), saved);
        assert!(
            std::fs::read(&saved[0]).unwrap() == snapshot,
            "backup changed during ordinary restart"
        );
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(read_history(&client, session.id).await, history);
        let Output::QueuedTurn(turn) = execute(
            &client,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Resume the preserved history".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        let resumed = finished(&client, session.id, turn.id).await;
        assert_eq!(resumed.runs.last().unwrap().status, Status::Completed);
        assert_eq!(&resumed.entries[..history.entries.len()], history.entries);
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        assert!(
            server
                .authorization
                .lock()
                .unwrap()
                .iter()
                .all(|value| value.as_deref() == Some("Bearer schema-fixture-credential"))
        );
        drop(client);
        node.shutdown().await.unwrap();
        assert_eq!(backups(&profile), saved);
        assert!(
            profile_files(&profile) == files,
            "ordinary restart changed unrelated profile files"
        );
        controller.close().await.unwrap();
    }
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn read_history(client: &Client, session: SessionId) -> Page {
    let Output::Conversation(page) = execute(
        client,
        Command::ReadConversation {
            session,
            before: None,
            limit: 100,
        },
    )
    .await
    else {
        panic!("history expected")
    };
    page.page
}

async fn finished(client: &Client, session: SessionId, turn: TurnId) -> Page {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let page = read_history(client, session).await;
            if page.runs.iter().any(|run| {
                run.turn == turn
                    && !matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping,
                    )
            }) {
                return page;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("history completion deadline")
}

fn open(path: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn rows(database: &rusqlite::Connection, sql: &str) -> Vec<Vec<rusqlite::types::Value>> {
    let mut query = database.prepare(sql).unwrap();
    let columns = query.column_count();
    query
        .query_map([], |row| {
            (0..columns)
                .map(|index| row.get(index))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn backups(profile: &Path) -> Vec<PathBuf> {
    let directory = profile.join("storage/backups");
    if !directory.exists() {
        return Vec::new();
    }
    entries(&directory)
        .into_iter()
        .map(|name| directory.join(name))
        .collect()
}

fn profile_files(profile: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(profile: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(profile).unwrap();
            if relative == Path::new("storage").join("backups")
                || ["node.sqlite3", "node.sqlite3-wal", "node.sqlite3-shm"]
                    .iter()
                    .any(|name| relative == Path::new("storage").join(name))
            {
                continue;
            }
            let metadata = entry.file_type().unwrap();
            if metadata.is_dir() {
                collect(profile, &path, files);
            } else if metadata.is_file() {
                files.insert(relative.to_owned(), std::fs::read(path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    collect(profile, profile, &mut files);
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
