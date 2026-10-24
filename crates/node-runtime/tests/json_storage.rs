//! Optional defaults restore without replay; incompatible records preserve stored bytes.
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{conversation::History, plugin::Info, *};
use serde_json::json;
use std::{fs, path::Path, time::Duration};
use turso::{Connection, Database, params};

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

async fn database(profile: &Path) -> (Database, Connection) {
    open(&profile.join("storage/node.turso"), true).await
}

async fn close(database: Database, connection: Connection) {
    connection
        .prepare("PRAGMA wal_checkpoint(TRUNCATE)")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap();
    drop(connection);
    drop(database);
}

/// Compare original JSON bytes, not normalized public projections or database pages.
async fn retained(profile: &Path) -> Vec<(String, Vec<u8>)> {
    let (_database, db) = database(profile).await;
    let sources = [
        (
            "events",
            "SELECT CAST(cursor AS TEXT),body FROM events ORDER BY cursor",
        ),
        (
            "agent_events",
            "SELECT session || ':' || id,body FROM agent_events ORDER BY session,id",
        ),
        (
            "session_config",
            "SELECT session || ':' || revision,config FROM session_revisions ORDER BY session,revision",
        ),
        (
            "session_roles",
            "SELECT session || ':' || revision,roles FROM session_revisions ORDER BY session,revision",
        ),
        (
            "session_profile",
            "SELECT session || ':' || revision,profile FROM session_revisions WHERE profile IS NOT NULL ORDER BY session,revision",
        ),
        (
            "requests",
            "SELECT hex(caller) || ':' || id,CAST(result AS BLOB) FROM requests WHERE result IS NOT NULL ORDER BY caller,id",
        ),
        ("providers", "SELECT id,body FROM providers ORDER BY id"),
        (
            "frozen_providers",
            "SELECT turn,provider FROM agent_runs WHERE provider IS NOT NULL ORDER BY turn",
        ),
        ("plugins", "SELECT name,body FROM plugins ORDER BY name"),
    ];
    let mut bodies = Vec::new();
    for (source, sql) in sources {
        let mut rows = db.query(sql, ()).await.unwrap();
        while let Some(row) = rows.next().await.unwrap() {
            let key = row.get::<String>(0).unwrap();
            let body = row.get::<Vec<u8>>(1).unwrap();
            bodies.push((format!("{source}:{key}"), body));
        }
    }
    bodies
}

async fn execute(client: &Client, command: Command) -> Result<Output, &'static str> {
    tokio::time::timeout(
        Duration::from_secs(20),
        client.execute(client.prepare(command)),
    )
    .await
    .map_err(|_| "read deadline exceeded")?
    .map_err(|_| "read failed")
}

#[derive(PartialEq, Eq)]
struct Observed {
    snapshot: Snapshot,
    plugins: Vec<Info>,
    sessions: Vec<Session>,
    history: Vec<History>,
}

async fn observe(client: &Client) -> Result<Observed, &'static str> {
    let Output::Snapshot(snapshot) = execute(client, Command::Snapshot).await? else {
        return Err("snapshot response expected");
    };
    let Output::Plugins(summaries) = execute(client, Command::ListPlugins).await? else {
        return Err("plugin list response expected");
    };
    let mut plugins = Vec::new();
    for summary in summaries {
        let Output::Plugin(info) = execute(
            client,
            Command::ReadPlugin {
                name: summary.name.clone(),
            },
        )
        .await?
        else {
            return Err("plugin response expected");
        };
        if info.summary.name != summary.name {
            return Err("plugin identity changed");
        }
        plugins.push(info);
    }
    let mut sessions = Vec::new();
    let mut history = Vec::new();
    for session in &snapshot.sessions {
        let Output::Session(read) = execute(
            client,
            Command::ReadSession {
                session: session.id,
            },
        )
        .await?
        else {
            return Err("session response expected");
        };
        if read != *session {
            return Err("session read differs from snapshot");
        }
        let page = tokio::time::timeout(
            Duration::from_secs(20),
            client.read_conversation(session.id, None, 20),
        )
        .await
        .map_err(|_| "history deadline exceeded")?
        .map_err(|_| "history read failed")?;
        if !page.missing.is_empty() || page.page.next_before.is_some() {
            return Err("history was not fully read");
        }
        sessions.push(read);
        history.push(page);
    }
    Ok(Observed {
        snapshot,
        plugins,
        sessions,
        history,
    })
}

mod recovery {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn restores_defaults_and_canonical_receipts() {
        for remote in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let project_path = directory.path().join("project");
            fs::create_dir(&project_path).unwrap();
            let profile = directory.path().join("node");
            let controller_path = directory.path().join("controller");
            let node = Node::start(&profile).await.unwrap();
            let controller = Link::controller(&controller_path, NetworkScope::default())
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
            let registration = client.prepare(Command::RegisterProject {
                name: "Storage fixture".into(),
                path: project_path.to_str().unwrap().into(),
            });
            let Output::Project(project) = client.execute(registration.clone()).await.unwrap()
            else {
                panic!("project response expected")
            };
            let creation = client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    features: Default::default(),
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    options: Default::default(),
                    permission: Permission::Ask,
                    credential: None,
                }),
            });
            let Output::Session(session) = client.execute(creation.clone()).await.unwrap() else {
                panic!("session response expected")
            };
            drop(client);
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();

            let (handle, db) = open(&profile.join("storage/node.turso"), false).await;
            let body: Vec<u8> = db
                .prepare("SELECT config FROM session_revisions WHERE session=?1 AND revision=1")
                .await
                .unwrap()
                .query_row([session.id.to_string()])
                .await
                .and_then(|row| row.get(0))
                .unwrap();
            let mut config: serde_json::Value = serde_json::from_slice(&body).unwrap();
            config.as_object_mut().unwrap().remove("effort");
            config["future_configuration"] = json!({ "ordinary": true });
            db.execute(
                "UPDATE session_revisions SET config=?1 WHERE session=?2 AND revision=1",
                params![serde_json::to_vec(&config).unwrap(), session.id.to_string()],
            )
            .await
            .unwrap();
            close(handle, db).await;
            let before = retained(&profile).await;

            let node = Node::start(&profile).await.unwrap();
            let controller = Link::controller(&controller_path, NetworkScope::default())
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
            let restored = observe(&client).await.unwrap();
            assert_eq!(restored.sessions.len(), 1);
            assert!(
                restored.sessions[0].config == session.config,
                "safe defaults changed the effective configuration"
            );
            assert_eq!(restored.snapshot.projects.len(), 1);
            for _ in 0..2 {
                assert_eq!(
                    client.execute(creation.clone()).await.unwrap(),
                    Output::Session(session.clone())
                );
                assert_eq!(
                    client.execute(registration.clone()).await.unwrap(),
                    Output::Project(project.clone())
                );
            }
            let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await.unwrap()
            else {
                panic!("snapshot response expected")
            };
            assert_eq!(snapshot.projects.len(), 1);
            assert_eq!(snapshot.projects[0].id, project.id);
            assert_eq!(snapshot.sessions.len(), 1);
            assert_eq!(snapshot.sessions[0].id, session.id);
            drop(client);
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
            assert!(
                retained(&profile).await == before,
                "JSON reads or receipt recovery rewrote stored bodies"
            );
        }
    }

    #[tokio::test]
    async fn rejects_incompatible_archives_during_updates() {
        for source in ["events.body", "requests.result"] {
            let directory = tempfile::tempdir().unwrap();
            let profile = directory.path().join("node");
            let node = Node::start(&profile).await.unwrap();
            let client = Client::new(node.local());
            let creation = client.prepare(Command::CreateSession {
                project: None,
                worktree: None,
                config: Some(SessionConfig {
                    features: Default::default(),
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    options: Default::default(),
                    permission: Permission::Ask,
                    credential: None,
                }),
            });
            let Output::Session(session) = client.execute(creation.clone()).await.unwrap() else {
                panic!("session expected")
            };
            drop(client);
            node.shutdown().await.unwrap();
            let path = profile.join("storage/node.turso");
            let (handle, db) = open(&path, false).await;
            let value = json!({"kind":"future_record","data":{"token":"diagnostic-secret-marker"}});
            match source {
                "events.body" => {
                    db.execute("BEGIN CONCURRENT", ()).await.unwrap();
                    let cursor = db.prepare("INSERT INTO counters(namespace,value) VALUES('events',1) ON CONFLICT(namespace) DO UPDATE SET value=value+1 RETURNING value").await.unwrap().query_row(()).await.unwrap().get::<i64>(0).unwrap();
                    db.execute(
                        "INSERT INTO events(cursor,body) VALUES(?1,?2)",
                        (cursor, serde_json::to_vec(&value).unwrap()),
                    )
                    .await
                    .unwrap();
                    db.execute("COMMIT", ()).await.unwrap();
                }
                "requests.result" => {
                    assert_eq!(
                        db.execute(
                            "UPDATE requests SET result=?1 WHERE id=?2",
                            params![json!({"Ok":value}).to_string(), creation.id.to_string()],
                        )
                        .await
                        .unwrap(),
                        1
                    );
                }
                _ => unreachable!(),
            }
            // Full archive validation runs during updates, before any replay or writes.
            db.execute_batch("DROP INDEX events_kind").await.unwrap();
            let pending = RequestId::new();
            assert_eq!(
                db.execute(
                    "INSERT INTO requests(caller,id,body,status) SELECT caller,?1,?2,'admitted' FROM requests WHERE id=?3",
                    params![pending.to_string(), b"retained-pending-request".as_slice(), creation.id.to_string()],
                ).await
                .unwrap(),
                1
            );
            close(handle, db).await;
            let before = fs::read(&path).unwrap();
            let bodies = retained(&profile).await;
            let error = Node::start(&profile)
                .await
                .err()
                .expect("incompatible profile accepted");
            let error = error.to_string();
            assert!(
                error.contains(&format!("invalid stored JSON in {source}")),
                "{error}"
            );
            assert!(error.contains("existing data was not changed"), "{error}");
            assert!(!error.contains("diagnostic-secret-marker"), "{error}");
            assert_eq!(
                fs::read(&path).unwrap(),
                before,
                "rejected startup changed storage bytes"
            );
            assert!(
                retained(&profile).await == bodies,
                "rejected startup changed stored JSON"
            );
            let (_handle, db) = database(&profile).await;
            let stored: (String, String) = db
                .prepare("SELECT id,worktree FROM sessions")
                .await
                .unwrap()
                .query_row(())
                .await
                .and_then(|row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap();
            assert_eq!(
                stored,
                (session.id.to_string(), session.worktree.to_string())
            );
            let completed: String = db
                .prepare("SELECT status FROM requests WHERE id=?1")
                .await
                .unwrap()
                .query_row([creation.id.to_string()])
                .await
                .and_then(|row| row.get(0))
                .unwrap();
            assert_eq!(completed, "completed");
            let pending: (String, Vec<u8>, Option<String>) = db
                .prepare("SELECT status,body,result FROM requests WHERE id=?1")
                .await
                .unwrap()
                .query_row([pending.to_string()])
                .await
                .and_then(|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .unwrap();
            assert_eq!(
                pending,
                (
                    "admitted".into(),
                    b"retained-pending-request".to_vec(),
                    None
                )
            );
        }
    }

    #[tokio::test]
    async fn unknown_permission_preserves_storage() {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        node.shutdown().await.unwrap();
        let path = profile.join("storage/node.turso");
        let (handle, db) = open(&path, false).await;
        let config = json!({ "provider": ProviderId::new(), "model": "fixture", "mode": "code", "permission": "future_permission" });
        db.execute(
            "INSERT INTO defaults(singleton,revision,config) VALUES(1,1,?1)",
            [serde_json::to_vec(&config).unwrap()],
        )
        .await
        .unwrap();
        close(handle, db).await;
        let before = fs::read(&path).unwrap();
        assert!(
            Node::start(&profile).await.is_err(),
            "unknown permission must not become execution authority"
        );
        assert!(
            fs::read(&path).unwrap() == before,
            "rejected permission changed the database"
        );
    }
}
