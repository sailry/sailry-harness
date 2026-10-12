//! Generic artifact contracts run through captured tools on both transports.
use super::*;
use sailry_protocol::{artifact, plugin};
use serde_json::{Value, json};
use std::fs;

#[path = "../../src/backup/tests/server.rs"]
#[allow(dead_code)]
mod backup_server;

#[path = "artifact/declarations.rs"]
mod declarations;
#[path = "artifact/lifecycle.rs"]
mod lifecycle;
#[path = "artifact/preview.rs"]
mod preview;

const PACKAGE: &str = "document-fixture";

async fn install(fixture: &process::Fixture, revision: u64, template: &str) -> plugin::Info {
    install_at(
        &fixture.client,
        fixture.session.worktree,
        &fixture.root,
        revision,
        template,
        &["chart", "fixture-accent"],
    )
    .await
}

async fn install_at(
    client: &Client,
    worktree: WorktreeId,
    project: &std::path::Path,
    revision: u64,
    template: &str,
    classes: &[&str],
) -> plugin::Info {
    let root = project.join("document-package");
    fs::create_dir_all(root.join("dev.sailry.platform/view")).unwrap();
    fs::write(root.join("dev.sailry.platform/view/index.html"), template).unwrap();
    fs::write(
        root.join("dev.sailry.platform/view/theme.css"),
        "section { padding: 1rem }",
    )
    .unwrap();
    fs::write(
        root.join("dev.sailry.platform/view/main.js"),
        "document.documentElement.dataset.fixture='ready';",
    )
    .unwrap();
    fs::write(
        root.join("plugin.json"),
        json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":PACKAGE,"version":"1.0.0","extensions":{"dev.sailry.platform":{
                "api_version":"v1","actions":["artifacts.read","artifacts.write"],
                "artifact_view":{"template":"dev.sailry.platform/view/index.html",
                    "styles":["dev.sailry.platform/view/theme.css"],
                    "scripts":["dev.sailry.platform/view/main.js"],"libraries":["echarts"],
                    "allowed_classes":classes},
                "tools":[
                    {"name":"write","description":"Write a document","operation":"artifacts.write"},
                    {"name":"read","description":"Read a document","operation":"artifacts.read"},
                    {"name":"list","description":"List documents","operation":"artifacts.list"},
                    {"name":"revisions","description":"List document revisions","operation":"artifacts.revisions"}
                ]
            }}
        })
        .to_string(),
    )
    .unwrap();
    let Output::Plugin(info) = client
        .execute(client.prepare(Command::InstallPlugin {
            worktree,
            path: "document-package".into(),
            name: PACKAGE.into(),
            expected_revision: revision,
            expected_digest: None,
        }))
        .await
        .unwrap()
    else {
        panic!("plugin expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

fn draft(id: Option<ArtifactId>, revision: Option<u32>, title: &str) -> Value {
    json!({"id":id,"expected_revision":revision,"title":title,"summary":"Compare the options",
        "html":"<section class='fixture-accent lead' onclick='bad()'><h1>Options</h1><a href='https://invalid.test'>External</a><pre class='chart'>{\"type\":\"bar\",\"data\":[{\"label\":\"A\",\"value\":1}]}</pre><script>bad()</script></section>"})
}

async fn submit(fixture: &process::Fixture, text: &str) -> TurnId {
    let Output::Session(session) = fixture
        .client
        .execute(fixture.client.prepare(Command::ReadSession {
            session: fixture.session.id,
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    let Output::QueuedTurn(turn) = fixture
        .client
        .execute(fixture.client.prepare(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: text.into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn.id
}

fn cards(page: &Page) -> Vec<artifact::Summary> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| {
            let Part::ToolResult { result, .. } = part else {
                return None;
            };
            let content: tool::Content =
                serde_json::from_value(result.get("sailry_content")?.clone()).ok()?;
            Some(content.blocks)
        })
        .flatten()
        .filter_map(|block| match block {
            tool::Block::Artifact(value) => Some(value),
            _ => None,
        })
        .collect()
}

async fn projected(
    fixture: &process::Fixture,
    id: ArtifactId,
    revision: u32,
) -> conversation::Snapshot {
    let mut subscription = fixture
        .client
        .subscribe_conversation(fixture.session.id)
        .await
        .unwrap();
    let Update::ConversationSnapshot(snapshot) = subscription.next().await.unwrap() else {
        panic!("conversation snapshot expected")
    };
    assert!(
        snapshot
            .artifacts
            .items
            .iter()
            .any(|value| value.id == id && value.revision == revision)
    );
    snapshot
}

#[tokio::test]
async fn preserves_metadata_read_authority() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![(
            plugin_tool(PACKAGE, "write"),
            draft(None, None, "Observed"),
        )])
        .await;
        let fixture = process::Fixture::new(remote, &server).await;
        let package = install(&fixture, 0, "<html><body>Observer template</body></html>").await;
        let turn = submit(&fixture, "Create an observed document").await;
        finished(&fixture.client, fixture.session.id, turn).await;
        let document = fixture
            .client
            .list_artifacts(fixture.session.id, None, 1)
            .await
            .unwrap()
            .items
            .remove(0);
        let scope = plugin::Context {
            invocation: None,
            turn: None,
            surface: plugin::desktop::Surface::Workspace,
            package: package.summary.reference(),
            worktree: Some(fixture.session.worktree),
            session: Some(fixture.session.id),
        };
        let read = |scope| {
            fixture
                .client
                .prepare(Command::ListArtifactRevisions {
                    id: document.id,
                    before: None,
                    limit: 1,
                    session: None,
                })
                .with_plugin(scope)
        };
        let Output::ArtifactRevisions(page) =
            fixture.client.execute(read(scope.clone())).await.unwrap()
        else {
            panic!("artifact revisions expected")
        };
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, document.id);

        let mut outside = scope.clone();
        outside.session = Some(SessionId::new());
        assert_eq!(
            fixture
                .client
                .execute(read(outside))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let mut changed = scope.clone();
        changed.package.digest = "0".repeat(64);
        assert_eq!(
            fixture
                .client
                .execute(read(changed))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );

        let source = fixture.root.join("observer-package");
        fs::create_dir(&source).unwrap();
        fs::write(
            source.join("plugin.json"),
            json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                "name":"observer-fixture","extensions":{"dev.sailry.platform":{
                    "api_version":"v1","actions":[]}}})
            .to_string(),
        )
        .unwrap();
        let Output::Plugin(ungranted) = fixture
            .client
            .execute(fixture.client.prepare(Command::InstallPlugin {
                worktree: fixture.session.worktree,
                path: "observer-package".into(),
                name: "observer-fixture".into(),
                expected_revision: 0,
                expected_digest: None,
            }))
            .await
            .unwrap()
        else {
            panic!("plugin expected")
        };
        assert!(ungranted.issues.is_empty(), "{:?}", ungranted.issues);
        let mut missing = scope.clone();
        missing.package = ungranted.summary.reference();
        assert_eq!(
            fixture
                .client
                .execute(read(missing))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::SetPluginEnabled {
                name: PACKAGE.into(),
                expected_revision: package.summary.revision,
                enabled: false,
            }))
            .await
            .unwrap();
        assert_eq!(
            fixture
                .client
                .execute(read(scope.clone()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadSession {
                session: fixture.session.id,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture
            .client
            .execute(fixture.client.prepare(Command::RemoveSession {
                session: session.id,
                expected_revision: session.revision,
            }))
            .await
            .unwrap();
        assert_eq!(
            fixture.client.execute(read(scope)).await.unwrap_err().code,
            ErrorCode::NotFound
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn supports_bound_connections() {
    use sailry_protocol::{connection::Resource, database, ssh};
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![]).await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        install(&fixture, 0, "<html><body>Connection template</body></html>").await;
        let path = fixture.directory.path().join("bound.sqlite3");
        drop(rusqlite::Connection::open(&path).unwrap());
        let Output::DatabaseProfile(database) = fixture
            .client
            .execute(fixture.client.prepare(Command::SaveDatabase {
                profile: database::Profile {
                    id: DatabaseId::new(),
                    revision: 0,
                    name: "Bound database".into(),
                    connection: database::Connection::Sqlite {
                        path: path.to_str().unwrap().into(),
                    },
                    read_only: true,
                    sharing: None,
                },
                expected_revision: 0,
                password: None,
            }))
            .await
            .unwrap()
        else {
            panic!("database expected")
        };
        let Output::SshProfile(ssh) = fixture
            .client
            .execute(fixture.client.prepare(Command::SaveSsh {
                profile: ssh::Profile {
                    id: SshId::new(),
                    revision: 0,
                    name: "Bound SSH".into(),
                    host: "127.0.0.1".into(),
                    port: 22,
                    username: "fixture".into(),
                    authentication: ssh::Authentication::Password,
                    host_key: None,
                    sharing: None,
                },
                expected_revision: 0,
                credential: Some(ssh::Credential::Password {
                    password: Secret::new("fixture-password".into()),
                }),
            }))
            .await
            .unwrap()
        else {
            panic!("SSH expected")
        };
        for resource in [Resource::Database(database.id), Resource::Ssh(ssh.id)] {
            let mut config = fixture.session.config.clone();
            config.resource = Some(resource);
            config.assistant = None;
            config.mode = WorkMode::Plan;
            let Output::Session(session) = fixture
                .client
                .execute(fixture.client.prepare(Command::CreateSession {
                    project: None,
                    worktree: None,
                    config: Some(config),
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            fixture.session = session;
            server.replace_tools(vec![(
                plugin_tool(PACKAGE, "write"),
                draft(None, None, "Connection brief"),
            )]);
            let turn = submit(&fixture, "Write the connection summary").await;
            let page = questions::finish_plan(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{page:?}"
            );
            let documents = cards(&page);
            let document = documents.last().unwrap();
            let snapshot = configuration::snapshot(&fixture.client).await;
            assert_eq!(
                snapshot
                    .turns
                    .iter()
                    .find(|capture| capture.id == turn)
                    .unwrap()
                    .config
                    .resource,
                Some(resource)
            );
            {
                let requests = server.requests.lock().unwrap();
                let request = requests.last().unwrap();
                let tools = request["tools"].as_array().unwrap();
                let names = tools
                    .iter()
                    .filter_map(|tool| tool.pointer("/function/name").and_then(Value::as_str))
                    .collect::<Vec<_>>();
                for name in ["write", "read", "list", "revisions"] {
                    assert!(
                        names.contains(&plugin_tool(PACKAGE, name).as_str()),
                        "{names:?}"
                    );
                }
                assert!(
                    !names.contains(&plugin_tool("files", "read_file").as_str()),
                    "{names:?}"
                );
            }
            server.replace_tools(vec![
                (plugin_tool(PACKAGE, "read"), json!({"id":document.id})),
                (plugin_tool(PACKAGE, "list"), json!({"limit":64})),
                (plugin_tool(PACKAGE, "revisions"), json!({"id":document.id})),
            ]);
            let turn = submit(&fixture, "Inspect the saved connection summary").await;
            let page = questions::finish_plan(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{page:?}"
            );
            for name in ["read", "list", "revisions"] {
                let result = page
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .find_map(|part| {
                        if let Part::ToolResult {
                            name: tool, result, ..
                        } = part
                            && *tool == plugin_tool(PACKAGE, name)
                        {
                            Some(result)
                        } else {
                            None
                        }
                    })
                    .unwrap();
                assert!(result.get("error").is_none(), "{result:?}");
                if name == "read" {
                    assert_eq!(result["artifact"]["id"], json!(document.id));
                    assert!(result.get("view").is_none());
                } else {
                    assert!(
                        result["items"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|value| value["id"] == json!(document.id))
                    );
                }
            }
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn reads_inherited_publications() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![(
            plugin_tool(PACKAGE, "write"),
            draft(None, None, "First"),
        )])
        .await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        let package = install(&fixture, 0, "<html><body>History template</body></html>").await;
        let first = submit(&fixture, "Write the first revision").await;
        let page = finished(&fixture.client, fixture.session.id, first).await;
        let original = cards(&page).remove(0);
        let Output::Session(branch) = fixture
            .client
            .execute(fixture.client.prepare(Command::ForkConversation {
                session: fixture.session.id,
                through: first,
                expected_revision: fixture.session.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected")
        };
        server.replace_tools(vec![(
            plugin_tool(PACKAGE, "write"),
            draft(Some(original.id), Some(1), "Second"),
        )]);
        let second = submit(&fixture, "Revise the source document").await;
        finished(&fixture.client, fixture.session.id, second).await;
        let inherited = fixture
            .client
            .read_artifact_in(branch.id, original.id, None)
            .await
            .unwrap();
        assert_eq!(inherited.artifact, original);
        assert_eq!(inherited.latest_revision, 1);
        let inventory = fixture
            .client
            .conversation_assets(
                branch.id,
                conversation::assets::Query {
                    kind: Some(conversation::assets::Kind::Artifact),
                    before: None,
                    limit: 20,
                },
            )
            .await
            .unwrap();
        assert!(inventory.groups.iter().any(|group| {
            group
                .items
                .contains(&conversation::assets::Target::Document(original.clone()))
        }));
        assert_eq!(
            fixture
                .client
                .read_artifact_in(branch.id, original.id, Some(2))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let revisions = fixture
            .client
            .list_artifact_revisions_in(branch.id, original.id, None, 1)
            .await
            .unwrap();
        assert_eq!(
            revisions
                .items
                .iter()
                .map(|value| value.revision)
                .collect::<Vec<_>>(),
            [1]
        );
        assert_eq!(revisions.next_before, None);
        let context = plugin::Context {
            invocation: None,
            turn: None,
            surface: plugin::desktop::Surface::Workspace,
            package: package.summary.reference(),
            worktree: Some(branch.worktree),
            session: Some(branch.id),
        };
        let Output::Artifact(scoped) = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::ReadArtifact {
                        id: original.id,
                        revision: Some(1),
                        session: Some(branch.id),
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("artifact expected")
        };
        assert_eq!(scoped, inherited);
        assert_eq!(
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::ReadArtifact {
                            id: original.id,
                            revision: Some(1),
                            session: Some(fixture.session.id),
                        })
                        .with_plugin(context)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let Output::Session(unrelated) = fixture
            .client
            .execute(fixture.client.prepare(Command::CreateSession {
                project: None,
                worktree: None,
                config: Some(fixture.session.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_eq!(
            fixture
                .client
                .read_artifact_in(unrelated.id, original.id, Some(1))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let owner = fixture.session.clone();
        fixture.session = branch.clone();
        server.replace_tools(vec![(
            plugin_tool(PACKAGE, "write"),
            draft(Some(original.id), Some(1), "Forbidden rewrite"),
        )]);
        let attempt = submit(&fixture, "Attempt to rewrite the inherited document").await;
        let page = finished(&fixture.client, branch.id, attempt).await;
        assert!(page.entries.iter().filter(|entry| entry.turn == attempt).flat_map(|entry| &entry.parts).any(|part| matches!(part, Part::ToolResult { result, .. } if result.get("error").is_some())));
        assert_eq!(
            fixture
                .client
                .read_artifact(original.id, None)
                .await
                .unwrap()
                .artifact
                .revision,
            2
        );
        let Output::Rewound(rewind) = fixture
            .client
            .execute(fixture.client.prepare(Command::RewindConversation {
                session: owner.id,
                through: Some(first),
                expected_head: second,
                expected_revision: 1,
            }))
            .await
            .unwrap()
        else {
            panic!("rewind expected")
        };
        assert_eq!(
            fixture
                .client
                .read_artifact_in(rewind.backup.id, original.id, Some(2))
                .await
                .unwrap()
                .artifact
                .revision,
            2
        );
        let Output::Session(current) = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::ReadSession { session: owner.id }),
            )
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture
            .client
            .execute(fixture.client.prepare(Command::RemoveSession {
                session: current.id,
                expected_revision: current.revision,
            }))
            .await
            .unwrap();
        assert_eq!(
            fixture
                .client
                .read_artifact(original.id, None)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            fixture
                .client
                .read_artifact_in(branch.id, original.id, None)
                .await
                .unwrap(),
            inherited
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::RemoveSession {
                session: rewind.backup.id,
                expected_revision: rewind.backup.revision,
            }))
            .await
            .unwrap();
        assert_eq!(
            fixture
                .client
                .read_artifact_in(branch.id, original.id, None)
                .await
                .unwrap(),
            inherited
        );
        let Output::Session(branch) = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::ReadSession { session: branch.id }),
            )
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture
            .client
            .execute(fixture.client.prepare(Command::RemoveSession {
                session: branch.id,
                expected_revision: branch.revision,
            }))
            .await
            .unwrap();
        assert_eq!(
            fixture
                .client
                .read_artifact(original.id, None)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert!(
            !fixture
                .node
                .profile()
                .join("artifacts")
                .join(owner.id.to_string())
                .exists()
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_documents_when_stopped() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![
            (plugin_tool(PACKAGE, "write"), draft(None, None, "Retained")),
            ("ask_user".into(), json!({"prompt":"Choose an option","input":{"kind":"text","multiline":false,"max_bytes":256}})),
            (plugin_tool(PACKAGE, "write"), draft(None, None, "Not executed")),
        ])
        .await;
        let fixture = process::Fixture::new(remote, &server).await;
        install(&fixture, 0, "<html><body>Template</body></html>").await;
        let turn = submit(&fixture, "Write the document, then ask for a choice").await;
        let (page, _) = questions::pending(&fixture.client, fixture.session.id).await;
        let documents = cards(&page);
        assert_eq!(documents.len(), 1);
        let saved = fixture
            .client
            .read_artifact(documents[0].id, None)
            .await
            .unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::StopTurn { turn }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
        assert_eq!(cards(&page), documents);
        assert_eq!(
            fixture
                .client
                .read_artifact(saved.artifact.id, None)
                .await
                .unwrap(),
            saved
        );
        assert_eq!(
            fixture
                .client
                .list_artifacts(fixture.session.id, None, 64)
                .await
                .unwrap()
                .items,
            documents
        );
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn excludes_writes_from_delegated_tools() {
    for remote in [false, true] {
        let parent = Server::tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"review","task":"Inspect the options"}),
        )])
        .await;
        let child = Server::start(false).await;
        let fixture = delegation::fixture::Fixture::start(remote, parent, child).await;
        install_at(
            &fixture.client,
            fixture.session.worktree,
            &fixture.root(),
            0,
            "<html><body>Template</body></html>",
            &["chart", "fixture-accent"],
        )
        .await;
        let turn = fixture.push("Delegate the analysis").await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        {
            let requests = fixture.child.requests.lock().unwrap();
            assert!(!requests.is_empty());
            for request in requests.iter() {
                let names = request["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|tool| tool.pointer("/function/name").and_then(Value::as_str))
                    .collect::<Vec<_>>();
                assert!(
                    !names.contains(&plugin_tool(PACKAGE, "write").as_str()),
                    "{names:?}"
                );
                assert!(
                    names.contains(&plugin_tool(PACKAGE, "read").as_str()),
                    "{names:?}"
                );
            }
        }
        assert!(
            fixture
                .client
                .list_artifacts(fixture.session.id, None, 64)
                .await
                .unwrap()
                .items
                .is_empty()
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn persists_plan_documents_and_conflicts() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![(
            plugin_tool(PACKAGE, "write"),
            draft(None, None, "First"),
        )])
        .await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        let first_package = install(&fixture, 0, "<html><body>First template</body></html>").await;
        planning::configure(&mut fixture, WorkMode::Plan).await;
        let mut stream = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        assert!(matches!(
            stream.next().await.unwrap(),
            Update::ConversationSnapshot(_)
        ));
        let turn = submit(&fixture, "Produce the requested document").await;
        let page = questions::finish_plan(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        let first = fixture
            .client
            .list_artifacts(fixture.session.id, None, 1)
            .await
            .unwrap();
        assert_eq!(first.items.len(), 1, "{page:?}");
        let original = first.items[0].clone();
        assert_eq!(original.revision, 1);
        assert_eq!(original.package, first_package.summary.reference());
        assert_eq!(cards(&page).as_slice(), std::slice::from_ref(&original));
        assert!(
            page.approvals
                .iter()
                .all(|approval| approval.source == ApprovalSource::Storage)
        );
        let mut content = fixture
            .client
            .read_artifact(original.id, None)
            .await
            .unwrap();
        assert!(!content.html.contains("onclick"));
        assert!(!content.html.contains("https://"));
        assert!(!content.html.contains("<script"));
        assert!(content.html.contains("class=\"chart\""));
        assert!(content.html.contains("class=\"fixture-accent\""));
        assert!(!content.html.contains("lead"));
        assert!(!content.warnings.is_empty());
        assert_eq!(
            content.view.as_ref().unwrap().template,
            "<html><body>First template</body></html>"
        );
        assert_eq!(
            content.view.as_ref().unwrap().libraries,
            [artifact::Library::Echarts]
        );
        let announced = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Update::ConversationFrame(frame) = stream.next().await.unwrap()
                    && let Change::Artifact(value) = frame.change
                {
                    break value;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(announced, original);
        drop(stream);
        projected(&fixture, original.id, 1).await;
        assert!(!fixture.root.join("artifacts").exists());
        assert!(
            fixture
                .node
                .profile()
                .join("artifacts")
                .join(fixture.session.id.to_string())
                .join(original.id.to_string())
                .join("1.html")
                .is_file()
        );

        let second_package = install(
            &fixture,
            first_package.summary.revision,
            "<html><body>Second template</body></html>",
        )
        .await;
        assert_ne!(second_package.summary.digest, first_package.summary.digest);
        planning::configure(&mut fixture, WorkMode::Code).await;
        // Simulate an HTML publication whose metadata transaction did not commit.
        fs::write(
            fixture
                .node
                .profile()
                .join("artifacts")
                .join(fixture.session.id.to_string())
                .join(original.id.to_string())
                .join("2.html"),
            "interrupted revision",
        )
        .unwrap();
        server.replace_tools(vec![(
            plugin_tool(PACKAGE, "write"),
            draft(Some(original.id), Some(1), "Second"),
        )]);
        let revised = submit(&fixture, "Revise the existing document").await;
        let page = finished(&fixture.client, fixture.session.id, revised).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        let current = fixture
            .client
            .read_artifact(original.id, None)
            .await
            .unwrap();
        assert_eq!(current.artifact.revision, 2);
        assert_eq!(current.artifact.package, second_package.summary.reference());
        assert_eq!(current.artifact.created_at, original.created_at);
        assert_eq!(
            current.view.as_ref().unwrap().template,
            "<html><body>Second template</body></html>"
        );
        content.latest_revision = 2;
        assert_eq!(
            fixture
                .client
                .read_artifact(original.id, Some(1))
                .await
                .unwrap(),
            content
        );
        assert_eq!(
            cards(&page)
                .iter()
                .map(|card| card.revision)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        projected(&fixture, original.id, 2).await;
        let newest = fixture
            .client
            .list_artifact_revisions(original.id, None, 1)
            .await
            .unwrap();
        assert_eq!(newest.items[0].revision, 2);
        assert_eq!(newest.next_before, Some(2));
        let oldest = fixture
            .client
            .list_artifact_revisions(original.id, newest.next_before, 1)
            .await
            .unwrap();
        assert_eq!(oldest.items[0].revision, 1);
        assert_eq!(oldest.next_before, None);

        server.replace_tools(vec![(
            plugin_tool(PACKAGE, "read"),
            json!({"id":original.id,"revision":1}),
        )]);
        let inspected = submit(&fixture, "Read the previous document revision").await;
        let page = finished(&fixture.client, fixture.session.id, inspected).await;
        let result = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .find_map(|part| {
                if let Part::ToolResult { name, result, .. } = part
                    && *name == plugin_tool(PACKAGE, "read")
                {
                    Some(result)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(result["html"], content.html);
        assert_eq!(result["latest_revision"], 2);
        assert!(result.get("view").is_none());

        server.replace_tools(vec![(
            plugin_tool(PACKAGE, "write"),
            draft(Some(original.id), Some(1), "Stale"),
        )]);
        let rejected = submit(&fixture, "Attempt an outdated edit").await;
        let page = finished(&fixture.client, fixture.session.id, rejected).await;
        assert!(page.entries.iter().flat_map(|entry|&entry.parts).any(|part|matches!(part,
            Part::ToolResult {result,..} if result.pointer("/error/code")==Some(&json!("revision_conflict")))));
        assert_eq!(
            fixture
                .client
                .read_artifact(original.id, None)
                .await
                .unwrap(),
            current
        );

        // The controller cannot promote an arbitrary command into tool authority.
        let forged = fixture.client.prepare(Command::WriteArtifact(
            serde_json::from_value(draft(None, None, "Forged")).unwrap(),
        ));
        assert_eq!(
            fixture.client.execute(forged).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        let scope = plugin::Context {
            invocation: None,
            turn: Some(revised),
            surface: plugin::desktop::Surface::Workspace,
            package: second_package.summary.reference(),
            worktree: Some(fixture.session.worktree),
            session: Some(fixture.session.id),
        };
        let stopped = fixture
            .client
            .prepare(Command::WriteArtifact(
                serde_json::from_value(draft(None, None, "Stopped")).unwrap(),
            ))
            .with_plugin(scope);
        assert!(fixture.client.execute(stopped).await.is_err());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn restores_revisions_and_removes_session_files() {
    use sailry_protocol::{
        backup::{Kind, Update as BackupUpdate},
        plugin::settings::SecretUpdate,
    };
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![(
            plugin_tool(PACKAGE, "write"),
            draft(None, None, "First"),
        )])
        .await;
        let fixture = process::Fixture::new(remote, &server).await;
        install(&fixture, 0, "<html><body>Restore template</body></html>").await;
        let turn = submit(&fixture, "Write the initial document").await;
        finished(&fixture.client, fixture.session.id, turn).await;
        let original = fixture
            .client
            .list_artifacts(fixture.session.id, None, 64)
            .await
            .unwrap()
            .items
            .remove(0);
        server.replace_tools(vec![
            (
                plugin_tool(PACKAGE, "write"),
                draft(Some(original.id), Some(1), "Revised"),
            ),
            (plugin_tool(PACKAGE, "write"), draft(None, None, "Another")),
        ]);
        let turn = submit(&fixture, "Revise and add a document").await;
        finished(&fixture.client, fixture.session.id, turn).await;
        let first = fixture
            .client
            .list_artifacts(fixture.session.id, None, 1)
            .await
            .unwrap();
        assert_eq!(first.items[0].title, "Another");
        assert_eq!(first.next_after, Some(first.items[0].id));
        let last = fixture
            .client
            .list_artifacts(fixture.session.id, first.next_after, 1)
            .await
            .unwrap();
        assert_eq!(last.items[0].id, original.id);
        assert_eq!(last.items[0].revision, 2);
        assert_eq!(last.next_after, None);
        let contents = [
            fixture
                .client
                .read_artifact(original.id, Some(1))
                .await
                .unwrap(),
            fixture
                .client
                .read_artifact(original.id, Some(2))
                .await
                .unwrap(),
        ];

        let Output::Session(branch) = fixture
            .client
            .execute(fixture.client.prepare(Command::ForkConversation {
                session: fixture.session.id,
                through: turn,
                expected_revision: fixture.session.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected")
        };
        fixture
            .client
            .execute(fixture.client.prepare(Command::RemoveSession {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
            }))
            .await
            .unwrap();
        assert_eq!(
            fixture
                .client
                .read_artifact(original.id, None)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        let destination = backup_server::Server::start(Kind::Webdav).await;
        let Output::BackupSettings(mut settings) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadBackupSettings))
            .await
            .unwrap()
        else {
            panic!("backup settings expected")
        };
        settings.destination = Some(destination.destination());
        let Output::BackupSettings(settings) = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::SaveBackupSettings(BackupUpdate {
                        settings,
                        identity: SecretUpdate::Replace(Secret::new("fixture-user".into())),
                        secret: SecretUpdate::Replace(Secret::new("fixture-password".into())),
                        session_token: SecretUpdate::Clear,
                        passphrase: SecretUpdate::Replace(Secret::new(
                            "fixture-artifact-passphrase".into(),
                        )),
                    })),
            )
            .await
            .unwrap()
        else {
            panic!("backup settings expected")
        };
        let Output::Backup(record) = fixture
            .client
            .execute(fixture.client.prepare(Command::ExportBackup {
                expected_revision: settings.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("backup expected")
        };
        let target = fixture
            .directory
            .path()
            .canonicalize()
            .unwrap()
            .join("restored");
        fixture
            .client
            .execute(fixture.client.prepare(Command::RestoreBackup {
                id: record.id,
                expected_revision: settings.revision,
                target: target.to_str().unwrap().into(),
                passphrase: None,
            }))
            .await
            .unwrap();
        let identity = fixture.node.id();
        fixture.node.shutdown().await.unwrap();
        // Restored identity starts only after the original owner has shut down.
        let node = Node::start(&target).await.unwrap();
        assert_eq!(node.id(), identity);
        let address = fixture
            .controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(address)
        } else {
            node.local()
        });
        for (index, content) in contents.iter().enumerate() {
            assert_eq!(
                &client
                    .read_artifact_in(branch.id, original.id, Some(index as u32 + 1))
                    .await
                    .unwrap(),
                content
            );
        }
        let mut stream = client.subscribe_conversation(branch.id).await.unwrap();
        let Update::ConversationSnapshot(snapshot) = stream.next().await.unwrap() else {
            panic!("conversation snapshot expected")
        };
        assert!(snapshot.artifacts.items.is_empty());
        assert_eq!(
            cards(&snapshot.page)
                .iter()
                .map(|value| value.revision)
                .collect::<Vec<_>>(),
            [1, 2, 1]
        );
        drop(stream);
        let Output::Session(session) = client
            .execute(client.prepare(Command::ReadSession { session: branch.id }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        // A malformed private workspace must not prevent document cleanup.
        let workspaces = target.join("workspaces");
        fs::create_dir_all(&workspaces).unwrap();
        let scratch = workspaces.join("sessions");
        if scratch.exists() {
            fs::rename(&scratch, workspaces.join("retained-sessions")).unwrap();
        }
        fs::write(&scratch, "obstruction").unwrap();
        client
            .execute(client.prepare(Command::RemoveSession {
                session: session.id,
                expected_revision: session.revision,
            }))
            .await
            .unwrap();
        assert!(
            !target
                .join("artifacts")
                .join(fixture.session.id.to_string())
                .exists()
        );
        assert_eq!(
            client
                .read_artifact(original.id, None)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadSession {
                    session: session.id
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(fs::read_to_string(scratch).unwrap(), "obstruction");
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
