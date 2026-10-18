use super::*;
use sailry_protocol::{
    ApprovalId, Command, NodeId, Output, Request, SessionId, TurnId,
    conversation::{Approval, ApprovalChoice, ApprovalSource, ApprovalState, Decision},
    plugin, tool,
};
use serde_json::json;

mod approvals {
    use super::*;

    #[test]
    fn retains_manual_decisions_without_grants() {
        for (state, source, choice) in [
            ("approved", "user", Some(ApprovalChoice::Once)),
            ("approved", "full", None),
            ("approved", "project", None),
            ("pending", "user", None),
            ("denied", "user", None),
        ] {
            let fixture = Fixture::new();
            let old = legacy_approval(state, source);
            let before = bytes(&old);
            fixture.insert("agent_approvals", &before);

            assert_eq!(fixture.body("agent_approvals"), before);
            assert!(reconcile(&fixture.db).unwrap());
            let after = fixture.body("agent_approvals");
            let approval: Approval = serde_json::from_slice(&after).unwrap();
            assert!(!approval.can_remember);
            assert_eq!(approval.choice, choice);
            assert!(approval.prefix.is_none());
            assert!(approval.reason.is_none());
            assert!(approval.access.is_none());
            assert!(approval.startup.is_none());
            let mut retained: Value = serde_json::from_slice(&after).unwrap();
            retained.as_object_mut().unwrap().remove("can_remember");
            retained.as_object_mut().unwrap().remove("choice");
            assert_eq!(retained, old);

            assert_eq!(fixture.storage("agent_approvals", "body"), "blob");

            assert!(!reconcile(&fixture.db).unwrap());
            assert_eq!(fixture.body("agent_approvals"), after);

            if approval.source == ApprovalSource::User && approval.state == ApprovalState::Approved
            {
                assert_eq!(
                    crate::store::agent::approvals::resolve(
                        &fixture.db,
                        approval.session,
                        approval.id,
                        Decision::Approve
                    )
                    .unwrap(),
                    approval
                );
            }
        }
    }

    #[test]
    fn replays_the_original_durable_receipt() {
        use crate::store::database::{Database, encode};
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3");
        let node = NodeId([21; 32]);
        let mut database = Database::open(&path, node, None).unwrap();
        let old = legacy_approval("approved", "user");
        let approval_id: ApprovalId = serde_json::from_value(old["id"].clone()).unwrap();
        let session: SessionId = serde_json::from_value(old["session"].clone()).unwrap();
        let request = Request::new(
            node,
            Command::ResolveApproval {
                session,
                approval: approval_id,
                decision: Decision::Approve,
            },
        );
        let request_body = encode(&request).unwrap();
        let old_result = bytes(&json!({"Ok":{"kind":"approval","data":old}}));
        database.connection.execute(
            "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)",
            params![node.0.as_slice(), request.id.to_string(), request_body, std::str::from_utf8(&old_result).unwrap()],
        ).unwrap();
        database
            .connection
            .execute_batch("BEGIN IMMEDIATE")
            .unwrap();
        assert!(reconcile(&database.connection).unwrap());
        database.connection.execute_batch("COMMIT").unwrap();
        let Output::Approval(approval) = database.admit(node, &request).unwrap().unwrap().unwrap()
        else {
            panic!("the original approval receipt must replay")
        };
        assert_eq!(approval.id, approval_id);
        assert_eq!(approval.session, session);
        assert_eq!(approval.choice, Some(ApprovalChoice::Once));
        assert!(!approval.can_remember);
        let retained: Vec<u8> = database
            .connection
            .query_row(
                "SELECT body FROM requests WHERE id=?1",
                [request.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retained, request_body);
        let mut changed = request.clone();
        changed.command = Command::ResolveApproval {
            session,
            approval: approval_id,
            decision: Decision::Deny,
        };
        assert_eq!(
            database.admit(node, &changed).unwrap_err().code,
            sailry_protocol::ErrorCode::Conflict
        );
        database.close().unwrap();
    }
}

mod packages {
    use super::*;

    #[test]
    fn retires_only_known_native_declarations() {
        for prepared in [false, true] {
            for source in ["plugins", "plugin_packages", "requests"] {
                let fixture = Fixture::new();
                let old = legacy_package(prepared);
                let stored = if source == "requests" {
                    json!({"Ok":{"kind":"plugin","data":old}})
                } else {
                    old.clone()
                };
                let before = bytes(&stored);
                fixture.insert(source, &before);
                assert!(reconcile(&fixture.db).unwrap());
                let after = fixture.body(source);
                let converted: Value = serde_json::from_slice(&after).unwrap();
                let converted = if source == "requests" {
                    &converted["Ok"]["data"]
                } else {
                    &converted
                };
                let info: plugin::Info = serde_json::from_value(converted.clone()).unwrap();
                let extension = info.extension.as_ref().unwrap();
                assert_eq!(extension.tools.len(), 2);
                assert!(extension.tools[0].operation.is_none());
                assert!(extension.tools[0].handler.is_none());
                assert_eq!(extension.tools[0].name, "export_pdf");
                assert_eq!(
                    converted["extension"]["tools"][1],
                    old["extension"]["tools"][1]
                );
                assert_eq!(converted["summary"], old["summary"]);
                assert_eq!(
                    converted["extension"]["actions"],
                    old["extension"]["actions"]
                );
                assert_eq!(info.issues.len(), 2);
                assert_eq!(info.issues[1].kind, plugin::IssueKind::InvalidExtensions);
                let field = if prepared {
                    "handler.operation"
                } else {
                    "operation"
                };
                assert!(info.issues[1].path.ends_with(&format!("tools[0].{field}")));
                let alias = crate::plugins::tools::alias(&info.summary.name, "export_pdf");
                assert!(crate::plugins::tools::operation(&info, &alias).is_none());
                let read = crate::plugins::tools::alias(&info.summary.name, "read_file");
                assert_eq!(
                    crate::plugins::tools::operation(&info, &read),
                    Some(tool::Operation::ReadFile)
                );
                let column = if source == "requests" {
                    "result"
                } else {
                    "body"
                };

                assert_eq!(
                    fixture.storage(source, column),
                    if source == "requests" { "text" } else { "blob" }
                );
                assert!(!reconcile(&fixture.db).unwrap());
                assert_eq!(fixture.body(source), after);
            }
        }
    }

    #[test]
    fn defers_unknown_operations_to_validation() {
        use crate::store::database::{Database, validation};
        for field in ["operation", "action"] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory
                .path()
                .canonicalize()
                .unwrap()
                .join("node.sqlite3");
            let database = Database::open(&path, NodeId([22; 32]), None).unwrap();
            let mut old = legacy_package(true);
            if field == "operation" {
                old["extension"]["tools"][1]["operation"] = json!("unknown.operation");
            } else {
                old["extension"]["actions"][0] = json!("unknown.action");
            }
            let before = bytes(&old);
            database.connection.execute("INSERT INTO plugins(name,revision,installed,body) VALUES('retired-office',7,1,?1)", [before.as_slice()]).unwrap();
            database
                .connection
                .execute_batch("BEGIN IMMEDIATE")
                .unwrap();
            assert!(reconcile(&database.connection).unwrap());
            let error = validation::stored(&database.connection)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("invalid stored JSON in plugins.body"),
                "{error}"
            );
            assert!(error.contains("existing data was not changed"), "{error}");
            assert!(!error.contains("unknown.operation") && !error.contains("unknown.action"));
            database.connection.execute_batch("ROLLBACK").unwrap();
            let retained: Vec<u8> = database
                .connection
                .query_row(
                    "SELECT body FROM plugins WHERE name='retired-office'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(retained, before);
            database.close().unwrap();
        }
    }
}

mod publication {
    use super::*;

    #[test]
    fn preserves_opaque_requests_and_payloads() {
        let fixture = Fixture::new();
        let old = json!({"Ok":{"kind":"plugin_result","data":{
            "can_remember":"user data", "handler":{"operation":"office.export"}
        }}});
        let before = bytes(&old);
        fixture.insert("requests", &before);
        let fingerprint = b"command-v1:retained-private-fingerprint";
        fixture
            .db
            .execute("UPDATE requests SET body=?1", [fingerprint.as_slice()])
            .unwrap();

        assert!(!reconcile(&fixture.db).unwrap());
        assert_eq!(fixture.body("requests"), before);
        let body: Vec<u8> = fixture
            .db
            .query_row("SELECT body FROM requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(body, fingerprint);
    }

    #[test]
    fn rolls_back_defaults_on_publication_failure() {
        let mut fixture = Fixture::new();
        let approval = bytes(&legacy_approval("approved", "user"));
        let package = bytes(&legacy_package(true));
        fixture.insert("agent_approvals", &approval);
        fixture.insert("plugins", &package);
        fixture.db.execute_batch("CREATE TRIGGER reject_defaults BEFORE UPDATE ON plugins BEGIN SELECT RAISE(ABORT,'fixture publication failure'); END").unwrap();
        let transaction = fixture.db.transaction().unwrap();
        assert!(reconcile(&transaction).is_err());
        let changed: bool = transaction
            .query_row(
                "SELECT json_extract(body,'$.can_remember')=0 FROM agent_approvals",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(changed);
        transaction.rollback().unwrap();
        assert_eq!(fixture.body("agent_approvals"), approval);
        assert_eq!(fixture.body("plugins"), package);
    }

    #[test]
    fn defers_syntax_errors_to_validation() {
        use crate::store::database::{Database, validation};
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3");
        let node = NodeId([23; 32]);
        let database = Database::open(&path, node, None).unwrap();
        let before = "{syntax-secret-marker";
        database.connection.execute("INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)", params![node.0.as_slice(), sailry_protocol::RequestId::new().to_string(), b"opaque-fingerprint".as_slice(), before]).unwrap();
        assert!(!reconcile(&database.connection).unwrap());
        let error = validation::stored(&database.connection)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("invalid stored JSON in requests.result, record 1, line 1"),
            "{error}"
        );
        assert!(error.contains("existing data was not changed"), "{error}");
        assert!(!error.contains("syntax-secret-marker"));
        let retained: String = database
            .connection
            .query_row("SELECT result FROM requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(retained, before);
        database.close().unwrap();
    }
}

mod artifacts {
    use super::*;

    #[test]
    fn fills_only_missing_metadata_kind() {
        for table in ["artifacts", "artifact_revisions"] {
            let fixture = Fixture::new();
            let old = json!({"id":"fixture", "title":"Retained metadata"});
            let before = bytes(&old);
            fixture.insert(table, &before);
            assert!(reconcile(&fixture.db).unwrap());
            let mut after: Value = serde_json::from_slice(&fixture.body(table)).unwrap();
            assert_eq!(
                after.as_object_mut().unwrap().remove("kind"),
                Some(json!("document"))
            );
            assert_eq!(after, old);
            assert_eq!(fixture.storage(table, "body"), "blob");
            assert!(!reconcile(&fixture.db).unwrap());
        }
        for kind in [json!("preview"), json!("unknown"), Value::Null] {
            let fixture = Fixture::new();
            let value = bytes(&json!({"id":"fixture","kind":kind}));
            fixture.insert("artifacts", &value);
            assert!(!reconcile(&fixture.db).unwrap());
            assert_eq!(fixture.body("artifacts"), value);
        }
    }

    #[test]
    fn rolls_back_both_metadata_tables() {
        let mut fixture = Fixture::new();
        let before = bytes(&json!({"id":"fixture"}));
        fixture.insert("artifacts", &before);
        fixture.insert("artifact_revisions", &before);
        fixture.db.execute_batch("CREATE TRIGGER reject_defaults BEFORE UPDATE ON artifact_revisions BEGIN SELECT RAISE(ABORT,'fixture publication failure'); END").unwrap();
        let transaction = fixture.db.transaction().unwrap();
        assert!(reconcile(&transaction).is_err());
        transaction.rollback().unwrap();
        assert_eq!(fixture.body("artifacts"), before);
        assert_eq!(fixture.body("artifact_revisions"), before);
    }
}

mod code {
    use super::*;

    #[test]
    fn fills_only_missing_editor_switch() {
        let fixture = Fixture::new();
        let mut old = serde_json::to_value(sailry_protocol::code::Settings::default()).unwrap();
        old.as_object_mut().unwrap().remove("editor_enabled");
        old["revision"] = json!(8);
        old["after_edit"] = json!(false);
        fixture.insert("code_settings", &bytes(&old));
        assert!(reconcile(&fixture.db).unwrap());
        let after = fixture.body("code_settings");
        let mut retained: Value = serde_json::from_slice(&after).unwrap();
        assert_eq!(
            retained.as_object_mut().unwrap().remove("editor_enabled"),
            Some(json!(false))
        );
        assert_eq!(retained, old);
        assert!(!reconcile(&fixture.db).unwrap());
        assert_eq!(fixture.body("code_settings"), after);
        for value in [json!(true), json!(false), json!("invalid")] {
            let fixture = Fixture::new();
            old["editor_enabled"] = value;
            let before = bytes(&old);
            fixture.insert("code_settings", &before);
            assert!(!reconcile(&fixture.db).unwrap());
            assert_eq!(fixture.body("code_settings"), before);
        }
    }

    #[test]
    fn backs_up_defaults_and_rejects_invalid_settings() {
        use crate::store::database::Database;
        for invalid in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory
                .path()
                .canonicalize()
                .unwrap()
                .join("node.sqlite3");
            let node = NodeId([26; 32]);
            Database::open(&path, node, None).unwrap().close().unwrap();
            let mut old = serde_json::to_value(sailry_protocol::code::Settings::default()).unwrap();
            old.as_object_mut().unwrap().remove("editor_enabled");
            old["revision"] = json!(8);
            if invalid {
                old["editor_enabled"] = json!("invalid");
            }
            let before = bytes(&old);
            let db = Connection::open(&path).unwrap();
            db.execute(
                "INSERT INTO code_settings(singleton,body) VALUES(1,?1)",
                [&before],
            )
            .unwrap();
            drop(db);
            if invalid {
                assert!(Database::open(&path, node, None).is_err());
                assert!(!directory.path().join("backups").exists());
                let db = Connection::open(&path).unwrap();
                let retained: Vec<u8> = db
                    .query_row("SELECT body FROM code_settings", [], |row| row.get(0))
                    .unwrap();
                assert_eq!(retained, before);
            } else {
                let database = Database::open(&path, node, None).unwrap();
                let settings = crate::store::code::settings(&database.connection).unwrap();
                assert!(!settings.editor_enabled);
                assert_eq!(settings.revision, 8);
                database.close().unwrap();
                let backups = std::fs::read_dir(directory.path().join("backups"))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                assert_eq!(backups.len(), 1);
                let mut saved = url::Url::from_file_path(backups[0].path()).unwrap();
                saved.query_pairs_mut().append_pair("immutable", "1");
                let db = Connection::open_with_flags(
                    saved.as_str(),
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                        | rusqlite::OpenFlags::SQLITE_OPEN_URI,
                )
                .unwrap();
                let retained: Vec<u8> = db
                    .query_row("SELECT body FROM code_settings", [], |row| row.get(0))
                    .unwrap();
                assert_eq!(retained, before);
                drop(db);
                assert_eq!(
                    std::fs::read_dir(directory.path().join("backups"))
                        .unwrap()
                        .count(),
                    1
                );
                Database::open(&path, node, None).unwrap().close().unwrap();
                assert_eq!(
                    std::fs::read_dir(directory.path().join("backups"))
                        .unwrap()
                        .count(),
                    1
                );
            }
        }
    }

    #[test]
    fn rolls_back_failed_defaults() {
        let mut fixture = Fixture::new();
        let mut old = serde_json::to_value(sailry_protocol::code::Settings::default()).unwrap();
        old.as_object_mut().unwrap().remove("editor_enabled");
        let before = bytes(&old);
        fixture.insert("code_settings", &before);
        fixture.db.execute_batch("CREATE TRIGGER reject_editor BEFORE UPDATE ON code_settings BEGIN SELECT RAISE(ABORT,'fixture editor default failure'); END").unwrap();
        let transaction = fixture.db.transaction().unwrap();
        assert!(reconcile(&transaction).is_err());
        transaction.rollback().unwrap();
        assert_eq!(fixture.body("code_settings"), before);
    }
}

mod catalog {
    use super::*;
    use sailry_client::Client;
    use sailry_link::{Link, NetworkScope};
    use sailry_protocol::{
        Permission, ProviderId, Secret, SessionConfig, WorkMode,
        conversation::{History, Provider, Status},
    };
    use std::{path::Path, time::Duration};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };

    #[tokio::test]
    async fn excludes_retired_tools_from_both_binders() {
        for remote in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().canonicalize().unwrap();
            let project = root.join("project");
            let profile = root.join("node");
            std::fs::create_dir(&project).unwrap();
            let node = crate::Node::start_empty(&profile).await.unwrap();
            node.shutdown().await.unwrap();
            let manifest = source(&project);
            let mut info = crate::plugins::Host::new(Some(profile.clone()))
                .install(&project, "package", "upgrade-fixture")
                .unwrap();
            assert!(info.issues.is_empty(), "{:?}", info.issues);
            assert_eq!(info.extension.as_ref().unwrap().tools.len(), 4);
            info.summary.revision = 1;
            info.summary.enabled = true;
            let reference = info.summary.reference();
            let mut old = serde_json::to_value(&info).unwrap();
            old["extension"]["tools"][0]["operation"] = json!("office.export");
            old["extension"]["tools"][1]["handler"]["operation"] = json!("office.export");
            let before = bytes(&old);
            let path = profile.join("storage/node.sqlite3");
            let db = Connection::open(&path).unwrap();
            db.execute(
                "INSERT INTO plugins(name,revision,installed,body) VALUES(?1,1,1,?2)",
                params![info.summary.name, before],
            )
            .unwrap();
            db.execute(
                "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
                params![info.summary.digest, info.summary.name, before],
            )
            .unwrap();
            drop(db);
            let node = crate::Node::start_empty(&profile).await.unwrap();
            let controller = Link::controller(root.join("controller"), NetworkScope::default())
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
            let Output::Plugin(converted) = execute(
                &client,
                Command::ReadPlugin {
                    name: info.summary.name.clone(),
                },
            )
            .await
            else {
                panic!("converted plugin expected")
            };
            assert_eq!(converted.summary.reference(), reference);
            let (endpoint, captured, server) = model().await;
            let provider: Provider = serde_json::from_value(json!({
                "id":ProviderId::new(),"revision":0,"name":"Upgrade fixture",
                "api":"chat_completions","authentication":"api_key","endpoint":endpoint,
                "enabled":true,"default_model":"fixture","credential":null,
                "models":[{"id":"fixture","context":16000,"output":1024,"tools":true,"reasoning":false,
                    "capabilities":{"tools":true,"reasoning":false},"default_effort":"default"}]
            })).unwrap();
            let Output::Provider(provider) = execute(
                &client,
                Command::SaveProvider {
                    expected_revision: 0,
                    provider,
                    secret: Some(Secret::new("isolated-upgrade-fixture".into())),
                },
            )
            .await
            else {
                panic!("fixture provider expected")
            };
            let Output::Project(project) = execute(
                &client,
                Command::RegisterProject {
                    name: "Upgrade fixture".into(),
                    path: project.to_str().unwrap().into(),
                },
            )
            .await
            else {
                panic!("fixture project expected")
            };
            let Output::Session(session) = execute(
                &client,
                Command::CreateSession {
                    project: Some(project.id),
                    worktree: None,
                    config: Some(SessionConfig {
                        assistant: None,
                        resource: None,
                        provider: provider.id,
                        model: "fixture".into(),
                        effort: Default::default(),
                        mode: WorkMode::Code,
                        options: Default::default(),
                        permission: Permission::Ask,
                        credential: provider.credential.clone(),
                    }),
                },
            )
            .await
            else {
                panic!("fixture session expected")
            };
            let Output::QueuedTurn(turn) = execute(
                &client,
                Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: "Inspect the available tools".into(),
                },
            )
            .await
            else {
                panic!("fixture turn expected")
            };
            let request = tokio::time::timeout(Duration::from_secs(10), captured)
                .await
                .unwrap()
                .unwrap();
            let names: Vec<_> = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|tool| tool["function"]["name"].as_str())
                .collect();
            for name in ["retired_native", "retired_script"] {
                let alias = crate::plugins::tools::alias(&info.summary.name, name);
                assert!(
                    !names.contains(&alias.as_str()),
                    "retired tool was registered: {name}"
                );
            }
            // Surviving registrations prove the real native and script binders
            // both ran, rather than an entirely unavailable provider catalog.
            for name in ["safe_native", "safe_script"] {
                let alias = crate::plugins::tools::alias(&info.summary.name, name);
                assert!(
                    names.contains(&alias.as_str()),
                    "surviving tool was omitted: {name}"
                );
            }
            let history = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let Output::Conversation(history) = execute(
                        &client,
                        Command::ReadConversation {
                            session: session.id,
                            before: None,
                            limit: 10,
                        },
                    )
                    .await
                    else {
                        panic!("canonical history expected")
                    };
                    if history
                        .page
                        .runs
                        .iter()
                        .any(|run| run.turn == turn.id && run.status == Status::Completed)
                    {
                        break history;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            completed(&history);
            assert_eq!(
                std::fs::read(root.join("project/package/plugin.json")).unwrap(),
                manifest
            );
            server.await.unwrap();
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
            let backups: Vec<_> = std::fs::read_dir(profile.join("storage/backups"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            assert_eq!(backups.len(), 1);
            let db = Connection::open_with_flags(
                &backups[0],
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let stored: Vec<u8> = db
                .query_row(
                    "SELECT body FROM plugin_packages WHERE digest=?1",
                    [&reference.digest],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(stored, before);
        }
    }

    fn completed(history: &History) {
        assert!(history.page.approvals.is_empty());
        assert!(history.page.entries.iter().any(|entry| entry.parts.iter().any(|part| {
            matches!(part, sailry_protocol::conversation::Part::Text(text) if text == "Available tools inspected")
        })));
        assert!(
            !history
                .page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| {
                    matches!(part, sailry_protocol::conversation::Part::ToolCall { .. })
                })
        );
    }

    async fn execute(client: &Client, command: Command) -> Output {
        client.execute(client.prepare(command)).await.unwrap()
    }

    fn source(project: &Path) -> Vec<u8> {
        let package = project.join("package");
        let entry = "dev.sailry.platform/host/main.js";
        std::fs::create_dir_all(package.join("dev.sailry.platform/host")).unwrap();
        std::fs::write(package.join(entry),
            "export function prepare(args) { return args; }\nexport function safe() { return {text:'Safe read'}; }\n"
        ).unwrap();
        let manifest = bytes(&json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name":"upgrade-fixture","version":"1.0.0",
            "extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["files.read","files.write"],
                "host":{"entry":entry,"resources":[entry],"handlers":["prepare","safe"]},
                "tools":[
                    {"name":"retired_native","description":"Inspect office","operation":"office.read"},
                    {"name":"retired_script","description":"Prepare office","handler":{"name":"prepare","parameters":{"type":"object"},"operation":"office.read"}},
                    {"name":"safe_native","description":"Read file","operation":"files.read"},
                    {"name":"safe_script","description":"Read metadata","handler":{"name":"safe","parameters":{"type":"object"},"read_only":true}}
                ]
            }}
        }));
        std::fs::write(package.join("plugin.json"), &manifest).unwrap();
        manifest
    }

    async fn model() -> (
        String,
        oneshot::Receiver<Value>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let (send, receive) = oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0; 4096];
            let end = loop {
                let count = stream.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0);
                bytes.extend_from_slice(&chunk[..count]);
                assert!(bytes.len() <= 2 * 1024 * 1024);
                if let Some(end) = bytes.windows(4).position(|chunk| chunk == b"\r\n\r\n") {
                    break end + 4;
                }
            };
            let header = std::str::from_utf8(&bytes[..end]).unwrap();
            let length: usize = header
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .unwrap()
                .1
                .trim()
                .parse()
                .unwrap();
            assert!(length <= 2 * 1024 * 1024);
            while bytes.len() < end + length {
                let count = stream.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0);
                bytes.extend_from_slice(&chunk[..count]);
            }
            let request = serde_json::from_slice(&bytes[end..end + length]).unwrap();
            send.send(request).unwrap();
            let event = json!({
                "id":"upgrade-answer","object":"chat.completion.chunk","created":0,"model":"fixture",
                "choices":[{"index":0,"delta":{"content":"Available tools inspected"},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":8,"completion_tokens":4,"total_tokens":12}
            });
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {event}\n\ndata: [DONE]\n\n").as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        });
        (endpoint, receive, task)
    }
}

#[test]
fn streams_all_rows_without_skipping_updates() {
    let fixture = Fixture::new();
    for row in [i64::MIN, -1, 0, 1, i64::MAX] {
        fixture
            .db
            .execute(
                "INSERT INTO code_settings(rowid,body) VALUES(?1,?2)",
                params![row, b"{}".as_slice()],
            )
            .unwrap();
    }
    let transaction = fixture.db.unchecked_transaction().unwrap();
    assert!(reconcile(&transaction).unwrap());
    let count: i64 = transaction
        .query_row(
            "SELECT count(*) FROM code_settings WHERE json_extract(body,'$.editor_enabled')=0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 5);
    assert!(!reconcile(&transaction).unwrap());
    transaction.rollback().unwrap();
    assert_eq!(
        fixture
            .db
            .query_row(
                "SELECT count(*) FROM code_settings WHERE body=X'7b7d'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        5
    );
}

struct Fixture {
    db: Connection,
}

impl Fixture {
    fn new() -> Self {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE agent_approvals(id TEXT PRIMARY KEY,body BLOB NOT NULL);
             CREATE TABLE plugins(body BLOB NOT NULL);
             CREATE TABLE plugin_packages(body BLOB NOT NULL);
             CREATE TABLE artifacts(body BLOB NOT NULL);
             CREATE TABLE artifact_revisions(body BLOB NOT NULL);
             CREATE TABLE code_settings(body BLOB NOT NULL);
             CREATE TABLE requests(body BLOB NOT NULL,result TEXT);",
        )
        .unwrap();
        Self { db }
    }

    fn insert(&self, table: &str, body: &[u8]) {
        match table {
            "requests" => {
                self.db
                    .execute(
                        "INSERT INTO requests(body,result) VALUES(X'00',?1)",
                        [std::str::from_utf8(body).unwrap()],
                    )
                    .unwrap();
            }
            "agent_approvals" => {
                let value: Value = serde_json::from_slice(body).unwrap();
                self.db
                    .execute(
                        "INSERT INTO agent_approvals(id,body) VALUES(?1,?2)",
                        params![value["id"].as_str().unwrap(), body],
                    )
                    .unwrap();
            }
            _ => {
                self.db
                    .execute(&format!("INSERT INTO {table}(body) VALUES(?1)"), [body])
                    .unwrap();
            }
        }
    }

    fn body(&self, table: &str) -> Vec<u8> {
        let column = if table == "requests" {
            "result"
        } else {
            "body"
        };
        self.db
            .query_row(
                &format!("SELECT CAST({column} AS BLOB) FROM {table}"),
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn storage(&self, table: &str, column: &str) -> String {
        self.db
            .query_row(
                &format!("SELECT typeof({column}) FROM {table}"),
                [],
                |row| row.get(0),
            )
            .unwrap()
    }
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec_pretty(value).unwrap()
}

fn legacy_approval(state: &str, source: &str) -> Value {
    json!({
        "id": ApprovalId::new(), "session": SessionId::new(), "turn": TurnId::new(),
        "entry":"canonical-tool-call", "index":0,"state":state,"source":source
    })
}

fn legacy_package(prepared: bool) -> Value {
    let mut declaration = json!({
        "name":"export_pdf", "description":"Export PDF", "operation":"office.export",
        "display":{"label":"Export PDF","input":{"summary":{"path":"/source"}}}
    });
    if prepared {
        declaration["operation"] = Value::Null;
        declaration["handler"] = json!({
            "name":"export_pdf", "parameters":{"type":"object","properties":{"source":{"type":"string"}}},
            "operation":"office.export","read_only":false,"flow":null,"result":null
        });
    }
    json!({
        "summary":{"name":"retired-office","revision":7,"digest":"retained-package-digest","settings_revision":3,"enabled":true},
        "extension":{"api_version":"v1","actions":["files.read","files.write"],"tools":[
            declaration, {"name":"read_file","operation":"files.read","description":"Read file"}
        ]},
        "issues":[{"path":"plugin.json.optional","kind":"ignored_manifest_field"}]
    })
}
