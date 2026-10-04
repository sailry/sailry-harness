//! Stored JSON is read tolerantly without rewriting history or replaying receipts.
use rusqlite::{Connection, OpenFlags, params};
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{conversation::History, plugin::Info, *};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

fn database(profile: &Path) -> Connection {
    Connection::open_with_flags(
        profile.join("storage/node.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("test database must be readable")
}

/// Compare original JSON bytes, not normalized public projections or database pages.
fn retained(profile: &Path) -> Vec<(String, Vec<u8>)> {
    let db = database(profile);
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
        let mut query = db.prepare(sql).unwrap();
        let rows = query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .unwrap();
        bodies.extend(rows.map(|row| {
            let (key, body) = row.unwrap();
            (format!("{source}:{key}"), body)
        }));
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

mod copied_profile {
    use super::*;

    fn regular_tree(directory: &Path) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            assert!(
                kind.is_file() || kind.is_dir(),
                "acceptance files must not redirect outside the copy"
            );
            if kind.is_dir() {
                regular_tree(&entry.path());
            }
        }
    }

    fn path() -> PathBuf {
        let supplied = PathBuf::from(
            std::env::var_os("SAILRY_TEST_STORED_PROFILE")
                .expect("SAILRY_TEST_STORED_PROFILE must select an isolated acceptance copy"),
        );
        assert!(
            !fs::symlink_metadata(&supplied)
                .unwrap()
                .file_type()
                .is_symlink(),
            "profile must not be a symlink"
        );
        let path = supplied.canonicalize().unwrap();
        let parent = path.parent().expect("profile parent expected");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("profile")
        );
        assert!(
            parent
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("sailry-json-audit.")),
            "isolated acceptance directory expected"
        );
        let mut roots = vec![std::env::temp_dir().canonicalize().unwrap()];
        #[cfg(unix)]
        if let Ok(root) = Path::new("/tmp").canonicalize() {
            roots.push(root);
        }
        assert!(
            roots
                .iter()
                .any(|root| parent.parent() == Some(root.as_path())),
            "profile must be inside a temporary directory"
        );
        let marker = path.join(".sailry-acceptance-copy");
        assert!(
            fs::symlink_metadata(&marker).unwrap().file_type().is_file(),
            "regular ownership marker expected"
        );
        assert_eq!(
            fs::read_to_string(marker).unwrap(),
            "isolated startup acceptance copy\n"
        );
        regular_tree(&path);
        assert!(
            !path.join("storage/node.sqlite3-wal").exists(),
            "closed database snapshot expected"
        );
        assert!(
            !path.join("storage/node.sqlite3-shm").exists(),
            "closed database snapshot expected"
        );
        let db = database(&path);
        for sql in [
            "SELECT count(*) FROM agent_pending",
            "SELECT count(*) FROM requests WHERE status!='completed'",
            "SELECT count(*) FROM agent_runs WHERE coalesce(json_extract(body,'$.status'),'unknown')!='completed'",
            "SELECT count(*) FROM terminals WHERE coalesce(json_extract(body,'$.status.kind'),'unknown')!='stopped'",
            "SELECT count(*) FROM dispatch_jobs",
            "SELECT count(*) FROM dispatch_schedules",
            "SELECT count(*) FROM dispatch_handlers",
            "SELECT count(*) FROM link_peers",
            "SELECT count(*) FROM removed_projects",
        ] {
            let count: i64 = db.query_row(sql, [], |row| row.get(0)).unwrap();
            assert_eq!(
                count, 0,
                "acceptance copy must have no pending execution or external peers"
            );
        }
        drop(db);
        path
    }

    async fn read(node: &Node) -> Result<(), &'static str> {
        let directory = tempfile::tempdir().map_err(|_| "controller directory failed")?;
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .map_err(|_| "controller startup failed")?;
        let checked = async {
            let invitation = node.link().invite().map_err(|_| "invitation failed")?;
            let address = controller
                .handle()
                .pair(invitation.ticket())
                .await
                .map_err(|_| "pairing failed")?;
            let local = observe(&Client::new(node.local())).await?;
            let remote = observe(&Client::new(controller.handle().remote(address))).await?;
            if local != remote {
                return Err("local and Iroh projections differ");
            }
            if local.sessions.len() != 1
                || local.plugins.len() != 19
                || local.snapshot.providers.len() != 3
            {
                return Err("copied catalog counts changed");
            }
            if local.history.len() != 1
                || local.history[0].page.runs.len() != 6
                || local.history[0].page.entries.is_empty()
            {
                return Err("copied six-turn history was not restored");
            }
            if local.history[0]
                .page
                .runs
                .iter()
                .any(|run| run.status != conversation::Status::Completed)
            {
                return Err("copied finished turns changed state");
            }
            Ok(())
        }
        .await;
        let closed = controller
            .close()
            .await
            .map_err(|_| "controller shutdown failed");
        checked?;
        closed
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires SAILRY_TEST_STORED_PROFILE pointing to the marked private profile copy"]
    async fn reads_without_rewriting() {
        let profile = path();
        let before = retained(&profile);
        let started = Node::start_with_network(&profile, NetworkScope::default()).await;
        assert!(started.is_ok(), "isolated profile startup failed");
        let node = started.unwrap();
        let checked = read(&node).await;
        let stopped = node.shutdown().await;
        assert!(stopped.is_ok(), "isolated profile shutdown failed");
        assert!(checked.is_ok(), "isolated local and Iroh reads failed");
        assert!(
            retained(&profile) == before,
            "stored JSON bytes were changed during startup or reads"
        );
    }
}

mod recovery {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn defaults_and_unknown_receipts_do_not_replay() {
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
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
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

            let db = Connection::open(profile.join("storage/node.sqlite3")).unwrap();
            let body: Vec<u8> = db
                .query_row(
                    "SELECT config FROM session_revisions WHERE session=?1 AND revision=1",
                    [session.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            let mut config: serde_json::Value = serde_json::from_slice(&body).unwrap();
            config.as_object_mut().unwrap().remove("effort");
            config["future_configuration"] = json!({ "ordinary": true });
            db.execute(
                "UPDATE session_revisions SET config=?1 WHERE session=?2 AND revision=1",
                params![serde_json::to_vec(&config).unwrap(), session.id.to_string()],
            )
            .unwrap();
            db.execute(
                "INSERT INTO events(body) VALUES(?1)",
                [
                    serde_json::to_vec(&json!({"kind":"future_event","data":{"inert":true}}))
                        .unwrap(),
                ],
            )
            .unwrap();
            let receipt =
                json!({ "Ok": { "kind": "future_output", "data": { "inert": true } } }).to_string();
            assert_eq!(
                db.execute(
                    "UPDATE requests SET result=?1 WHERE id=?2",
                    params![receipt, creation.id.to_string()]
                )
                .unwrap(),
                1
            );
            drop(db);
            let before = retained(&profile);

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
                let fault = client.execute(creation.clone()).await.unwrap_err();
                assert_eq!(fault.code, ErrorCode::Unavailable);
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
                retained(&profile) == before,
                "JSON reads or unsupported receipt recovery rewrote stored bodies"
            );
        }
    }

    #[tokio::test]
    async fn unknown_permission_preserves_storage() {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        node.shutdown().await.unwrap();
        let path = profile.join("storage/node.sqlite3");
        let db = Connection::open(&path).unwrap();
        let config = json!({ "provider": ProviderId::new(), "model": "fixture", "mode": "code", "permission": "future_permission" });
        db.execute(
            "INSERT INTO defaults(singleton,revision,config) VALUES(1,1,?1)",
            [serde_json::to_vec(&config).unwrap()],
        )
        .unwrap();
        drop(db);
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
