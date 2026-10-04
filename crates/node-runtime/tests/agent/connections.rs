use super::*;
use sailry_protocol::{
    connection::{Resource, Sharing},
    database,
};
use serde_json::json;

async fn save(
    client: &Client,
    path: &std::path::Path,
    sharing: Option<Sharing>,
) -> database::Profile {
    let Output::DatabaseProfile(profile) = client
        .execute(client.prepare(Command::SaveDatabase {
            profile: database::Profile {
                id: DatabaseId::new(),
                revision: 0,
                name: "Agent database".into(),
                connection: database::Connection::Sqlite {
                    path: path.to_str().unwrap().into(),
                },
                read_only: false,
                sharing,
            },
            expected_revision: 0,
            password: None,
        }))
        .await
        .unwrap()
    else {
        panic!("profile expected")
    };
    profile
}

#[tokio::test]
async fn scopes_database_approvals() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let path = directory.path().join("data.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE items(value INTEGER); INSERT INTO items VALUES (7)")
            .unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let profile = save(&client, &path, None).await;
        let other = save(&client, &path, Some(Sharing::Global)).await;
        let server = Server::tools(vec![
            (
                agent_support::plugin_tool("databases", "database_catalog"),
                json!({"connection":profile.id,"database":"main"}),
            ),
            (
                agent_support::plugin_tool("databases", "database_query"),
                json!({"connection":profile.id,"sql":"SELECT value FROM items"}),
            ),
            (
                agent_support::plugin_tool("databases", "database_query"),
                json!({"connection":profile.id,"sql":"INSERT INTO items VALUES (9)"}),
            ),
            (
                agent_support::plugin_tool("databases", "database_execute"),
                json!({"connection":profile.id,"sql":"INSERT INTO items VALUES (9)"}),
            ),
            (
                agent_support::plugin_tool("databases", "database_execute"),
                json!({"connection":profile.id,"sql":"DELETE FROM items"}),
            ),
        ])
        .await;
        let session = approvals::prepare(&client, &server, &root).await;
        let Output::Session(session) = client
            .execute(client.prepare(Command::BindConnectionSession {
                session: session.id,
                expected_revision: session.revision,
                resource: Resource::Database(profile.id),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_eq!(session.project, None);
        let snapshot = configuration::snapshot(&client).await;
        client
            .execute(client.prepare(Command::RemoveProject {
                expected: snapshot.projects[0].clone(),
            }))
            .await
            .unwrap();
        let snapshot = configuration::snapshot(&client).await;
        assert!(snapshot.projects.is_empty());
        assert!(snapshot.sessions.iter().any(|entry| entry.id == session.id));
        assert_eq!(
            snapshot
                .worktrees
                .iter()
                .find(|tree| tree.id == session.worktree)
                .unwrap()
                .project,
            None
        );
        let mut changed = session.config.clone();
        changed.resource = None;
        assert_eq!(
            client
                .execute(client.prepare(Command::SetSessionConfig {
                    session: session.id,
                    expected_revision: session.revision,
                    config: changed
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: Input {
                    text: "Inspect the referenced table".into(),
                    attachments: vec![],
                    references: vec![reference::Reference {
                        label: "items".into(),
                        target: reference::Target::Database {
                            connection: profile.id,
                            database: Some("main".into()),
                            table: Some(database::Table {
                                schema: "main".into(),
                                name: "items".into(),
                            }),
                        },
                    }],
                },
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let (_, approval) = approvals::pending(&client, session.id).await;
        let requests = server.requests.lock().unwrap().clone();
        let tools = requests[0]["tools"].as_array().unwrap();
        for denied in [
            "read_file",
            "write_file",
            "run_command",
            "ssh_run",
            "delegate_agent",
            "browser_click",
            "update_plan",
            "create_goal",
            "update_goal",
        ] {
            assert!(
                tools.iter().all(|tool| tool["function"]["name"] != denied
                    && tool["function"]["name"] != agent_support::plugin_tool("files", denied)
                    && tool["function"]["name"] != agent_support::plugin_tool("goals", denied)
                    && tool["function"]["name"] != agent_support::plugin_tool("progress", denied)
                    && tool["function"]["name"] != agent_support::plugin_tool("commands", denied)
                    && tool["function"]["name"] != agent_support::plugin_tool("browser", denied)
                    && tool["function"]["name"] != agent_support::plugin_tool("ssh", denied)
                    && tool["function"]["name"]
                        != agent_support::plugin_tool("delegation", denied)),
                "{denied}"
            );
        }
        let query = tools
            .iter()
            .find(|tool| {
                tool["function"]["name"]
                    == agent_support::plugin_tool("databases", "database_query")
            })
            .unwrap();
        assert_eq!(
            query["function"]["parameters"]["properties"]["connection"]["enum"],
            json!([profile.id])
        );
        assert!(
            !serde_json::to_string(&requests)
                .unwrap()
                .contains(&other.id.to_string())
        );
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: approval.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let (_, approval) = approvals::pending(&client, session.id).await;
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM items", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: approval.id,
                decision: Decision::Deny,
            }))
            .await
            .unwrap();
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.approvals.len(), 2);
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part, Part::ToolResult {result,..} if result["error"]["code"]=="permission_denied")));
        assert_eq!(
            page.runs[0].status,
            Status::Completed,
            "{:?}",
            page.runs[0].error
        );
        assert!(page.entries.iter().flat_map(|entry|&entry.parts).any(|part| match part {Part::ToolResult {result,..} => matches!(serde_json::from_value::<Output>(result.clone()),Ok(Output::DatabaseOutcome(database::Outcome::Query(result))) if result.rows==vec![vec![database::Value::Integer(7)]]),_=>false}));
        let projected = super::progress::observe(&client, session.id).await;
        let query = projected
            .calls
            .iter()
            .find(|call| call.name == agent_support::plugin_tool("databases", "database_query"))
            .unwrap();
        assert_eq!(
            query.presentation,
            sailry_protocol::tool::Presentation::Content
        );
        let table = query
            .display(&page)
            .unwrap()
            .output
            .as_ref()
            .unwrap()
            .table
            .as_ref()
            .unwrap()
            .render(
                query.arguments(&page).unwrap(),
                query.result(&page).unwrap(),
            )
            .unwrap();
        assert_eq!(table.columns, ["value"]);
        assert_eq!(table.rows, [vec!["7".to_owned()]]);
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM items", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn rechecks_sharing_after_approval() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let path = directory.path().join("data.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE items(value INTEGER)")
            .unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let mut profile = save(&client, &path, Some(Sharing::Global)).await;
        let server = Server::tools(vec![(
            agent_support::plugin_tool("databases", "database_execute"),
            json!({"connection":profile.id,"sql":"INSERT INTO items VALUES (1)"}),
        )])
        .await;
        let session = approvals::prepare(&client, &server, &root).await;
        profile.sharing = Some(Sharing::Projects(vec![session.project.unwrap()]));
        let Output::DatabaseProfile(mut profile) = client
            .execute(client.prepare(Command::SaveDatabase {
                expected_revision: profile.revision,
                profile,
                password: None,
            }))
            .await
            .unwrap()
        else {
            panic!("profile expected")
        };
        let turn = approvals::submit(&client, session.id).await;
        let (_, approval) = approvals::pending(&client, session.id).await;
        profile.sharing = None;
        client
            .execute(client.prepare(Command::SaveDatabase {
                expected_revision: profile.revision,
                profile,
                password: None,
            }))
            .await
            .unwrap();
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: approval.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let page = finished(&client, session.id, turn).await;
        assert_eq!(
            page.runs[0].status,
            Status::Completed,
            "{:?}",
            page.runs[0].error
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM items", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(page.entries.iter().flat_map(|entry|&entry.parts).any(|part| matches!(part,Part::ToolResult {result,..} if result["error"]["code"]=="permission_denied")));
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[cfg(unix)]
#[path = "../ssh/server.rs"]
#[expect(
    dead_code,
    reason = "The shared SSH fixture also supports deployment and counters exercised by SSH tests"
)]
mod ssh_peer;

#[cfg(unix)]
#[tokio::test]
async fn executes_private_ssh_references() {
    use sailry_protocol::ssh;
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let peer = ssh_peer::Server::start(directory.path().into(), 44).await;
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let Output::SshProfile(profile) = client
            .execute(client.prepare(Command::SaveSsh {
                expected_revision: 0,
                profile: ssh::Profile {
                    id: SshId::new(),
                    revision: 0,
                    name: "Agent SSH".into(),
                    host: "127.0.0.1".into(),
                    port: peer.port,
                    username: "fixture".into(),
                    authentication: ssh::Authentication::Password,
                    host_key: None,
                    sharing: Some(Sharing::Global),
                },
                credential: Some(ssh::Credential::Password {
                    password: Secret::new("isolated-ssh-password".into()),
                }),
            }))
            .await
            .unwrap()
        else {
            panic!("SSH profile expected")
        };
        client
            .execute(client.prepare(Command::TrustSsh {
                profile: profile.id,
                expected_revision: profile.revision,
                key: peer.key.clone(),
            }))
            .await
            .unwrap();
        let server = Server::tools(vec![
            (agent_support::plugin_tool("ssh", "ssh_run"),json!({"connection":profile.id,"command":"printf approved > effects; printf ready"})),
            (agent_support::plugin_tool("ssh", "ssh_transfer"),json!({"connection":profile.id,"path":"download.txt","remote_path":directory.path().join("effects"),"direction":"download"})),
            (agent_support::plugin_tool("ssh", "ssh_run"),json!({"connection":profile.id,"command":"printf denied >> effects"})),
        ]).await;
        let session = approvals::prepare(&client, &server, &root).await;
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: Input {
                    text: "Use the referenced connection".into(),
                    attachments: vec![],
                    references: vec![reference::Reference {
                        target: reference::Target::Ssh(profile.id),
                        label: "Agent SSH".into(),
                    }],
                },
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let (_, approval) = approvals::pending(&client, session.id).await;
        assert!(!directory.path().join("effects").exists());
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: approval.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let (_, approval) = approvals::pending(&client, session.id).await;
        assert_eq!(
            std::fs::read_to_string(directory.path().join("effects")).unwrap(),
            "approved"
        );
        assert!(!root.join("download.txt").exists());
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: approval.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let (_, approval) = approvals::pending(&client, session.id).await;
        assert_eq!(
            std::fs::read_to_string(root.join("download.txt")).unwrap(),
            "approved"
        );
        client
            .execute(client.prepare(Command::ResolveApproval {
                session: session.id,
                approval: approval.id,
                decision: Decision::Deny,
            }))
            .await
            .unwrap();
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(
            page.runs[0].status,
            Status::Completed,
            "{:?}",
            page.runs[0].error
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("effects")).unwrap(),
            "approved"
        );
        let requests = serde_json::to_string(&*server.requests.lock().unwrap()).unwrap();
        assert!(!requests.contains("isolated-ssh-password"));
        assert!(requests.contains(&profile.id.to_string()));
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        peer.close().await;
    }
}

#[path = "connections/ownership.rs"]
mod ownership;
