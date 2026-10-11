use super::*;
use sailry_protocol::{plugin, tool::Operation};
use serde_json::Value;

const PACKAGE: &str = "connection-services";

fn tool(name: &str) -> String {
    agent_support::plugin_tool(PACKAGE, name)
}

async fn install(fixture: &process::Fixture) -> plugin::Info {
    install_named(fixture, PACKAGE).await
}

async fn install_named(fixture: &process::Fixture, package: &str) -> plugin::Info {
    let path = fixture.root.join(format!("{package}-package"));
    std::fs::create_dir(&path).unwrap();
    let tools: Vec<_> = [
        ("database_connections", Operation::ListDatabaseConnections),
        ("database_save_connection", Operation::SaveDatabaseConnection),
        ("database_query", Operation::QueryDatabase),
        ("ssh_connections", Operation::ListSshConnections),
        ("ssh_save_connection", Operation::SaveSshConnection),
        ("ssh_run", Operation::RunSsh),
    ].into_iter().map(|(name, operation)| json!({"name":name,"operation":operation,"description":"Exercise the captured connection service"})).collect();
    std::fs::write(path.join("plugin.json"), serde_json::to_vec(&json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name":package,"version":"0.1.0","description":"Isolated connection service fixture",
        "extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["databases.read","databases.manage","ssh.read","ssh.control","ssh.manage"],"tools":tools}}
    })).unwrap()).unwrap();
    let Output::Plugin(info) = fixture
        .client
        .execute(fixture.client.prepare(Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: format!("{package}-package"),
            name: package.into(),
            expected_revision: 0,
            expected_digest: None,
        }))
        .await
        .unwrap()
    else {
        panic!("connection plugin expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

async fn configure(fixture: &mut process::Fixture, permission: Permission) {
    let mut config = fixture.session.config.clone();
    config.permission = permission;
    let Output::Session(session) = fixture
        .client
        .execute(fixture.client.prepare(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    fixture.session = session;
}

async fn submit(fixture: &process::Fixture) -> TurnId {
    let Output::QueuedTurn(turn) = fixture
        .client
        .execute(fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Use the connection services".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn.id
}

fn results(page: &Page) -> Vec<&Value> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| {
            if let Part::ToolResult { result, .. } = part {
                Some(result)
            } else {
                None
            }
        })
        .collect()
}

async fn databases(client: &Client) -> Vec<database::Profile> {
    let Output::DatabaseProfiles(profiles) = client
        .execute(client.prepare(Command::ListDatabases))
        .await
        .unwrap()
    else {
        panic!("database profiles expected")
    };
    profiles
}

async fn ssh_profiles(client: &Client) -> Vec<ssh::Profile> {
    let Output::SshProfiles(profiles) = client
        .execute(client.prepare(Command::ListSsh))
        .await
        .unwrap()
    else {
        panic!("SSH profiles expected")
    };
    profiles
}

#[tokio::test]
async fn bundled_tools_keep_saved_id() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        configure(&mut fixture, Permission::Full).await;
        assert!(databases(&fixture.client).await.is_empty());
        let path = fixture.directory.path().join("bundled.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE items(value INTEGER); INSERT INTO items VALUES (83)")
            .unwrap();
        let server = Server::result_tools(vec![
            (
                agent_support::plugin_tool("databases", "database_connections"),
                json!({}),
            ),
            (
                agent_support::plugin_tool("databases", "database_save_connection"),
                json!({"name":"Bundled SQLite", "engine":"sqlite", "path":path}),
            ),
            (
                agent_support::plugin_tool("databases", "database_query"),
                json!({"connection":"$connection", "sql":"SELECT value FROM items"}),
            ),
            (
                agent_support::plugin_tool("databases", "database_connections"),
                json!({}),
            ),
        ])
        .await;
        approvals::rules::route(&fixture.client, &fixture.session, &server.endpoint).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs[0].status,
            Status::Completed,
            "{:?}",
            page.runs[0].error
        );
        let output = results(&page);
        assert_eq!(output.len(), 4, "{output:?}");
        assert_eq!(output[0], &json!({"kind":"database_profiles", "data":[]}));
        let profiles = databases(&fixture.client).await;
        assert_eq!(profiles.len(), 1);
        assert!(profiles[0].read_only);
        assert_eq!(output[1]["kind"], "database_profile", "{output:?}");
        assert_eq!(output[1]["data"]["id"], profiles[0].id.to_string());
        assert_eq!(output[1]["data"]["credentials_required"], false);
        assert_eq!(
            output[2]["data"]["data"]["rows"][0][0]["value"], 83,
            "{output:?}"
        );
        assert_eq!(output[3]["data"][0]["id"], profiles[0].id.to_string());
        let requests = server.requests.lock().unwrap();
        let tools = requests[0]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|tool| tool["function"]["name"]
            == agent_support::plugin_tool("databases", "database_query")));
        drop(requests);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn creates_and_queries_in_one_turn() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        let info = install(&fixture).await;
        let other = install_named(&fixture, "other-connection-services").await;
        let path = fixture.directory.path().join("created.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE items(value INTEGER); INSERT INTO items VALUES (37)")
            .unwrap();
        let foreign = DatabaseId::new();
        let server = Server::result_tools(vec![
            (tool("database_connections"),json!({})),
            (tool("database_save_connection"),json!({"name":"Created SQLite","engine":"sqlite","path":path,"read_only":true})),
            (tool("database_query"),json!({"connection":"$connection","sql":"SELECT value FROM items","row_limit":20,"timeout_ms":3000})),
            (agent_support::plugin_tool(&other.summary.name,"database_query"),json!({"connection":"$connection","sql":"SELECT value FROM items","row_limit":20,"timeout_ms":3000})),
            (agent_support::plugin_tool(&other.summary.name,"database_connections"),json!({})),
            (tool("database_query"),json!({"connection":foreign,"sql":"SELECT value FROM items","row_limit":20,"timeout_ms":3000})),
            (tool("database_save_connection"),json!({"connection":"$connection","expected_revision":"$revision","name":"Renamed SQLite"})),
            (tool("database_connections"),json!({})),
        ]).await;
        approvals::rules::route(&fixture.client, &fixture.session, &server.endpoint).await;
        configure(&mut fixture, Permission::Ask).await;
        assert!(databases(&fixture.client).await.is_empty());
        let turn = submit(&fixture).await;
        let (_, first) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert!(databases(&fixture.client).await.is_empty());
        fixture
            .client
            .execute(fixture.client.prepare(Command::SaveDatabase {
                profile: database::Profile {
                    id: foreign,
                    revision: 0,
                    name: "Added after capture".into(),
                    sharing: Some(Sharing::Global),
                    connection: database::Connection::Sqlite {
                        path: path.to_str().unwrap().into(),
                    },
                    read_only: true,
                },
                expected_revision: 0,
                password: None,
            }))
            .await
            .unwrap();
        process::decide(&fixture.client, &first, Decision::Approve).await;
        let (_, second) = approvals::pending(&fixture.client, fixture.session.id).await;
        let profile = databases(&fixture.client)
            .await
            .into_iter()
            .find(|p| p.id != foreign)
            .unwrap();
        assert_eq!(profile.revision, 1);
        assert_eq!(profile.name, "Created SQLite");
        let (claimed, used) = approvals::rules::claimed(&fixture.node, &first);
        assert!(used);
        let mut forged = fixture
            .client
            .prepare(Command::SaveDatabase {
                profile: database::Profile {
                    name: "Forged save".into(),
                    ..profile.clone()
                },
                expected_revision: 1,
                password: None,
            })
            .with_plugin(plugin::Context {
                invocation: None,
                turn: Some(turn),
                surface: plugin::desktop::Surface::Workspace,
                package: info.summary.reference(),
                worktree: Some(fixture.session.worktree),
                session: Some(fixture.session.id),
            });
        forged.id = claimed;
        assert_eq!(
            fixture.client.execute(forged).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        // A fresh request cannot claim the already consumed canonical approval either.
        let forged = fixture
            .client
            .prepare(Command::SaveDatabase {
                profile: database::Profile {
                    name: "Forged save".into(),
                    ..profile.clone()
                },
                expected_revision: 1,
                password: None,
            })
            .with_plugin(plugin::Context {
                invocation: None,
                turn: Some(turn),
                surface: plugin::desktop::Surface::Workspace,
                package: info.summary.reference(),
                worktree: Some(fixture.session.worktree),
                session: Some(fixture.session.id),
            });
        assert_eq!(
            fixture.client.execute(forged).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        process::decide(&fixture.client, &second, Decision::Approve).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs[0].status,
            Status::Completed,
            "{:?}",
            page.runs[0].error
        );
        let output = results(&page);
        assert_eq!(output.len(), 8, "{output:?}");
        assert_eq!(output[0], &json!([]));
        assert_eq!(output[1]["id"], profile.id.to_string());
        assert_eq!(output[1]["credentials_required"], false);
        assert_eq!(output[2]["data"]["rows"][0][0]["value"], 37, "{output:?}");
        assert_eq!(
            output[3]["error"]["code"], "permission_denied",
            "{output:?}"
        );
        assert_eq!(output[4], &json!([]), "{output:?}");
        assert_eq!(
            output[5]["error"]["code"], "permission_denied",
            "{output:?}"
        );
        assert_eq!(output[6]["revision"], 2);
        assert_eq!(output[7].as_array().unwrap().len(), 1);
        assert_eq!(output[7][0]["name"], "Renamed SQLite");
        assert_eq!(databases(&fixture.client).await.len(), 2);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_credentials_and_reports_missing() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        install(&fixture).await;
        configure(&mut fixture, Permission::Full).await;
        let Output::DatabaseProfile(existing) = fixture
            .client
            .execute(fixture.client.prepare(Command::SaveDatabase {
                profile: database::Profile {
                    id: DatabaseId::new(),
                    revision: 0,
                    name: "Existing network".into(),
                    sharing: Some(Sharing::Global),
                    read_only: true,
                    connection: database::Connection::Network {
                        engine: database::Engine::Postgres,
                        host: "db.example.invalid".into(),
                        port: 5432,
                        database: "reports".into(),
                        username: "reader".into(),
                        tls: database::Tls::Require,
                    },
                },
                expected_revision: 0,
                password: Some(Secret::new("isolated-database-password".into())),
            }))
            .await
            .unwrap()
        else {
            panic!("database profile expected")
        };
        let server = Server::result_tools(vec![
            (tool("database_connections"),json!({})),
            (tool("database_save_connection"),json!({"connection":existing.id,"expected_revision":existing.revision,"name":"Renamed network"})),
            (tool("database_save_connection"),json!({"name":"Missing password","engine":"mysql","host":"db.example.invalid","port":3306,"username":"reader","database":"reports"})),
            (tool("database_connections"),json!({})),
            (tool("database_save_connection"),json!({"connection":existing.id,"expected_revision":existing.revision,"name":"Must conflict"})),
            (tool("database_save_connection"),json!({"name":"Must reject secret","engine":"mysql","host":"db.example.invalid","port":3306,"username":"reader","database":"reports","password":"model-password"})),
        ]).await;
        approvals::rules::route(&fixture.client, &fixture.session, &server.endpoint).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs[0].status,
            Status::Completed,
            "{:?}",
            page.runs[0].error
        );
        let output = results(&page);
        assert_eq!(output.len(), 6, "{output:?}");
        assert_eq!(output[0][0]["credentials_required"], false);
        assert_eq!(output[1]["revision"], 2);
        assert_eq!(output[1]["credentials_required"], false);
        assert_eq!(output[2]["credentials_required"], true);
        assert_eq!(output[2]["read_only"], true);
        assert_eq!(
            output[4]["error"]["code"], "revision_conflict",
            "{output:?}"
        );
        assert_eq!(output[5]["error"]["code"], "invalid_request", "{output:?}");
        let profiles = databases(&fixture.client).await;
        assert_eq!(profiles.len(), 2);
        let saved = profiles.iter().find(|p| p.id == existing.id).unwrap();
        assert_eq!(saved.connection, existing.connection);
        assert_eq!(saved.sharing, existing.sharing);
        let requests = serde_json::to_string(&*server.requests.lock().unwrap()).unwrap();
        assert!(!requests.contains("isolated-database-password"));
        let db = rusqlite::Connection::open_with_flags(
            fixture.node.profile().join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let password: String = db
            .query_row(
                "SELECT password FROM database_profiles WHERE id=?1",
                [existing.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(password == "isolated-database-password");
        let prepared: Vec<Vec<u8>> = db
            .prepare("SELECT prepared FROM agent_approvals")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(
            prepared
                .iter()
                .all(|body| !String::from_utf8_lossy(body).contains("isolated-database-password"))
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn requires_exact_first_use_consent() {
    use std::sync::atomic::Ordering;
    for remote in [false, true] {
        for (authentication, approve) in [
            (ssh::Authentication::KeyPath, false),
            (ssh::Authentication::KeyPath, true),
            (ssh::Authentication::Agent, false),
        ] {
            let server = Server::tools(vec![]).await;
            let mut fixture = process::Fixture::new(remote, &server).await;
            let info = install(&fixture).await;
            configure(&mut fixture, Permission::Full).await;
            let peer = ssh_peer::Server::start(fixture.directory.path().into(), 44).await;
            let encoded = ssh_peer::key(43)
                .to_openssh(russh::keys::ssh_key::LineEnding::LF)
                .unwrap();
            let path = fixture.directory.path().join("fixture-key");
            std::fs::write(&path, encoded.as_str()).unwrap();
            let mut save = json!({"name":"Created SSH","host":"127.0.0.1","port":peer.port,"username":"fixture","authentication":authentication});
            if authentication == ssh::Authentication::KeyPath {
                save["key_path"] = json!(path);
            }
            let server = Server::result_tools(vec![
                (tool("ssh_save_connection"),save),
                (tool("ssh_run"),json!({"connection":"$connection","command":"printf approved > effects; printf ready","timeout_ms":3000})),
                (tool("ssh_connections"),json!({})),
                (tool("ssh_save_connection"),json!({"connection":"$connection","expected_revision":"$revision","host_key":peer.key})),
                (tool("ssh_save_connection"),json!({"connection":"$connection","expected_revision":"$revision","password":"model-password"})),
            ]).await;
            approvals::rules::route(&fixture.client, &fixture.session, &server.endpoint).await;
            let turn = submit(&fixture).await;
            let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
            let profile = ssh_profiles(&fixture.client).await.pop().unwrap();
            assert_eq!(profile.authentication, authentication);
            assert!(profile.host_key.is_none());
            assert_eq!(profile.revision, 1);
            assert_eq!(
                approval.source,
                sailry_protocol::conversation::ApprovalSource::User
            );
            assert!(!approval.can_remember);
            let trust = approval
                .ssh
                .as_ref()
                .expect("exact host key consent required");
            assert_eq!(trust.profile, profile.id);
            assert_eq!(trust.expected_revision, 1);
            assert_eq!(trust.host, "127.0.0.1");
            assert_eq!(trust.port, peer.port);
            assert_eq!(trust.key, peer.key);
            assert!(!trust.changed);
            assert_eq!(peer.authentication.load(Ordering::SeqCst), 0);
            assert!(peer.commands.lock().unwrap().is_empty());
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ResolveApproval {
                        session: fixture.session.id,
                        approval: approval.id,
                        decision: Decision::ApproveSession,
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
            let context = plugin::Context {
                invocation: None,
                turn: Some(turn),
                surface: plugin::desktop::Surface::Workspace,
                package: info.summary.reference(),
                worktree: Some(fixture.session.worktree),
                session: Some(fixture.session.id),
            };
            let forged = fixture
                .client
                .prepare(Command::TrustSsh {
                    profile: profile.id,
                    expected_revision: 1,
                    key: peer.key.clone(),
                })
                .with_plugin(context);
            assert_eq!(
                fixture.client.execute(forged).await.unwrap_err().code,
                ErrorCode::PermissionDenied
            );
            process::decide(
                &fixture.client,
                &approval,
                if approve {
                    Decision::Approve
                } else {
                    Decision::Deny
                },
            )
            .await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs[0].status,
                Status::Completed,
                "{:?}",
                page.runs[0].error
            );
            let saved = ssh_profiles(&fixture.client).await.pop().unwrap();
            let output = results(&page);
            assert_eq!(output.len(), 5, "{output:?}");
            assert_eq!(output[3]["error"]["code"], "invalid_request");
            assert_eq!(output[4]["error"]["code"], "invalid_request");
            if approve {
                assert_eq!(saved.host_key, Some(peer.key.clone()));
                assert_eq!(saved.revision, 2);
                assert_eq!(peer.commands.lock().unwrap().len(), 1);
                assert_eq!(
                    std::fs::read_to_string(fixture.directory.path().join("effects")).unwrap(),
                    "approved"
                );
                assert!(peer.authentication.load(Ordering::SeqCst) > 0);
                assert_eq!(results(&page)[1]["stdout"], "ready");
            } else {
                assert!(saved.host_key.is_none());
                assert_eq!(saved.revision, 1);
                assert!(peer.commands.lock().unwrap().is_empty());
                assert_eq!(peer.authentication.load(Ordering::SeqCst), 0);
                assert!(!fixture.directory.path().join("effects").exists());
            }
            let requests = serde_json::to_string(&*server.requests.lock().unwrap()).unwrap();
            assert!(!requests.contains("BEGIN OPENSSH PRIVATE KEY"));
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
            peer.close().await;
        }
    }
}

#[tokio::test]
async fn rejects_changed_key_without_authentication() {
    use std::sync::atomic::Ordering;
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        install(&fixture).await;
        configure(&mut fixture, Permission::Full).await;
        let peer = ssh_peer::Server::start(fixture.directory.path().into(), 44).await;
        let Output::SshProfile(profile) = fixture
            .client
            .execute(fixture.client.prepare(Command::SaveSsh {
                profile: ssh::Profile {
                    id: SshId::new(),
                    revision: 0,
                    name: "Existing SSH".into(),
                    host: "127.0.0.1".into(),
                    port: peer.port,
                    username: "fixture".into(),
                    authentication: ssh::Authentication::Password,
                    host_key: None,
                    sharing: Some(Sharing::Global),
                },
                expected_revision: 0,
                credential: Some(ssh::Credential::Password {
                    password: Secret::new("isolated-ssh-password".into()),
                }),
            }))
            .await
            .unwrap()
        else {
            panic!("SSH profile expected")
        };
        let old = ssh::HostKey {
            algorithm: peer.key.algorithm.clone(),
            fingerprint: ssh_peer::key(45)
                .public_key()
                .fingerprint(russh::keys::HashAlg::Sha256)
                .to_string(),
        };
        fixture
            .client
            .execute(fixture.client.prepare(Command::TrustSsh {
                profile: profile.id,
                expected_revision: profile.revision,
                key: old.clone(),
            }))
            .await
            .unwrap();
        let server = Server::result_tools(vec![
            (tool("ssh_connections"),json!({})),
            (tool("ssh_save_connection"),json!({"connection":profile.id,"expected_revision":2,"name":"Renamed SSH"})),
            (tool("ssh_run"),json!({"connection":"$connection","command":"printf denied > effects","timeout_ms":3000})),
        ]).await;
        approvals::rules::route(&fixture.client, &fixture.session, &server.endpoint).await;
        let turn = submit(&fixture).await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        let saved = ssh_profiles(&fixture.client).await.pop().unwrap();
        assert_eq!(saved.name, "Renamed SSH");
        assert_eq!(saved.revision, 3);
        assert_eq!(saved.host_key, Some(old.clone()));
        let trust = approval.ssh.as_ref().unwrap();
        assert!(trust.changed);
        assert_eq!(trust.key, peer.key);
        assert_eq!(trust.expected_revision, 3);
        assert_eq!(peer.authentication.load(Ordering::SeqCst), 0);
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs[0].status,
            Status::Completed,
            "{:?}",
            page.runs[0].error
        );
        assert_eq!(ssh_profiles(&fixture.client).await[0].host_key, Some(old));
        assert!(peer.commands.lock().unwrap().is_empty());
        assert_eq!(peer.authentication.load(Ordering::SeqCst), 0);
        let requests = serde_json::to_string(&*server.requests.lock().unwrap()).unwrap();
        assert!(!requests.contains("isolated-ssh-password"));
        let db = rusqlite::Connection::open_with_flags(
            fixture.node.profile().join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let credential: String = db
            .query_row(
                "SELECT credential FROM ssh_profiles WHERE id=?1",
                [profile.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let credential: ssh::Credential = serde_json::from_str(&credential).unwrap();
        assert!(
            matches!(credential,ssh::Credential::Password { password } if password.expose() == "isolated-ssh-password")
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
        peer.close().await;
    }
}
