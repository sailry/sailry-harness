use super::*;
use sailry_protocol::plugin::{self, conversation::Binding};
use serde_json::{Value, json};
use std::path::Path;

#[path = "assistants/imports.rs"]
mod imports;
#[path = "assistants/resources.rs"]
mod resources;

const PACKAGE: &str = "assistant-notes";
const CONTEXT: &str = "Use the package note named review as the source for this assistant";
const REPLACEMENT: &str = "The replacement package has no tools";
const PROMPT: &str = "Summarize the notes";

fn alias(tool: &str) -> String {
    let digest = blake3::hash(PACKAGE.as_bytes()).to_hex();
    format!("plugin_{}_{tool}", &digest[..16])
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn install(fixture: &process::Fixture, revision: u64, replacement: bool) -> plugin::Info {
    let root = fixture.root.join("assistant");
    std::fs::create_dir_all(root.join(plugin::NAMESPACE)).unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/main.js"),
        "import { View, div } from 'gpui-kit'; export default class Notes extends View { render() { return div(); } }",
    ).unwrap();
    let tools = if replacement {
        vec![]
    } else {
        vec![
            json!({"kind":"package", "package":"files", "name":"read_file"}),
            json!({"kind":"builtin", "name":"load_skill"}),
            json!({"kind":"builtin", "name":"read_skill_resource"}),
            json!({"kind":"plugin", "name":"read_note"}),
        ]
    };
    std::fs::write(root.join("plugin.json"), json!({
        "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name": PACKAGE,
        "version": if replacement { "0.2.0" } else { "0.1.0" },
        "extensions": {"dev.sailry.platform": {
            "api_version":"v1",
            "scope":"desktop",
            "actions":["storage.read", "storage.write", "conversation.read", "conversation.control"],
            "tools":[
                {"name":"read_note", "operation":"storage.get"},
                {"name":"list_notes", "operation":"storage.list"}
            ],
            "desktop": {
                "entry":"dev.sailry.platform/main.js", "resources":["dev.sailry.platform/main.js"],
                "navigation":{"label":"Notes","icon":"reicon:files/file-text"},
                "conversations":[
                    {"id":"notes", "resource":"plugin", "tools":tools, "context":if replacement { REPLACEMENT } else { CONTEXT }},
                    {"id":"database", "resource":"database", "tools":[], "context":""}
                ]
            }
        }}
    }).to_string()).unwrap();
    std::fs::create_dir_all(root.join("skills/review/references")).unwrap();
    std::fs::write(
        root.join("skills/review/SKILL.md"),
        "---\nname: review\ndescription: Review the package notes\n---\nRead references/guide.md\n",
    )
    .unwrap();
    std::fs::write(
        root.join("skills/review/references/guide.md"),
        "Review guide",
    )
    .unwrap();
    load(fixture, revision).await
}

async fn load(fixture: &process::Fixture, revision: u64) -> plugin::Info {
    let Output::Plugin(info) = execute(
        &fixture.client,
        Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "assistant".into(),
            name: PACKAGE.into(),
            expected_revision: revision,
        },
    )
    .await
    else {
        panic!("plugin expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

async fn create(fixture: &process::Fixture, info: &plugin::Info) -> Session {
    let mut config = fixture.session.config.clone();
    config.assistant = Some(Binding {
        package: info.summary.reference(),
        id: "notes".into(),
    });
    config.permission = Permission::Full;
    let Output::Session(session) = execute(
        &fixture.client,
        Command::CreateSession {
            project: None,
            worktree: None,
            config: Some(config),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    assert_eq!(session.project, None);
    assert_eq!(session.config.resource, None);
    assert_eq!(
        session.config.assistant.as_ref().unwrap().package,
        info.summary.reference()
    );
    session
}

async fn unrelated(fixture: &process::Fixture) -> plugin::Info {
    let root = fixture.root.join("unrelated");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("plugin.json"),
        json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":"unrelated", "version":"0.1.0",
            "extensions":{"dev.sailry.platform":{
                "api_version":"v1", "scope":"host", "actions":["storage.read"],
                "tools":[{"name":"read_note", "operation":"storage.get"}]
            }}
        })
        .to_string(),
    )
    .unwrap();
    let Output::Plugin(info) = execute(
        &fixture.client,
        Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "unrelated".into(),
            name: "unrelated".into(),
            expected_revision: 0,
        },
    )
    .await
    else {
        panic!("plugin expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    assert!(info.summary.enabled);
    info
}

async fn reject_role(fixture: &process::Fixture, session: &Session) -> Session {
    let Output::Role(role) = execute(
        &fixture.client,
        Command::PutRole {
            role: role::Profile {
                appearance: None,
                id: RoleId::new(),
                revision: 0,
                key: "review".into(),
                name: "Review".into(),
                description: "Inspect the task".into(),
                model: None,
                max_turns: None,
                skills: vec![],
                instructions: "Review the notes".into(),
            },
            expected_revision: 0,
        },
    )
    .await
    else {
        panic!("role expected")
    };
    let Output::Session(session) = execute(
        &fixture.client,
        Command::SetSessionRoles {
            session: session.id,
            expected_revision: session.revision,
            roles: vec![role.reference()],
        },
    )
    .await
    else {
        panic!("session expected")
    };
    let fault = fixture
        .client
        .execute(fixture.client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: Input {
                text: PROMPT.into(),
                attachments: vec![],
                references: vec![reference::Reference {
                    target: reference::Target::Agent(role.reference()),
                    label: "Review".into(),
                }],
            },
        }))
        .await
        .unwrap_err();
    assert_eq!(fault.code, ErrorCode::PermissionDenied);
    session
}

async fn seed(fixture: &process::Fixture, session: &Session, info: &plugin::Info) {
    let request = fixture
        .client
        .prepare(Command::WritePluginValue {
            key: "review".into(),
            value: json!("Package-owned note"),
            expected_revision: 0,
        })
        .with_plugin(plugin::Context {
            invocation: None,
            turn: None,
            surface: plugin::desktop::Surface::Workspace,
            package: info.summary.reference(),
            worktree: Some(session.worktree),
            session: Some(session.id),
        });
    assert!(matches!(
        fixture.client.execute(request).await.unwrap(),
        Output::PluginValue(_)
    ));
}

async fn submit(client: &Client, session: &Session, queued: bool) -> QueuedTurn {
    let command = if queued {
        Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: PROMPT.into(),
        }
    } else {
        Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: PROMPT.into(),
        }
    };
    let Output::QueuedTurn(turn) = execute(client, command).await else {
        panic!("turn expected")
    };
    turn
}

fn model(request: &Value) {
    let tools: BTreeSet<_> = request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    for name in [
        plugin_tool("files", "read_file").as_str(),
        "load_skill",
        "read_skill_resource",
        "ask_user",
        "set_session_title",
        "compact_context",
    ] {
        assert!(tools.contains(name), "missing tool {name}");
    }
    assert!(tools.contains(alias("read_note").as_str()));
    for name in [
        plugin_tool("files", "write_file").as_str(),
        plugin_tool("delegation", "spawn_agent").as_str(),
    ] {
        assert!(!tools.contains(name), "unexpected tool {name}");
    }
    assert!(!tools.contains(plugin_tool("commands", "run_command").as_str()));
    assert!(!tools.contains(alias("list_notes").as_str()));
    assert!(
        request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| {
                message["role"] == "user" && message["content"].to_string().contains(CONTEXT)
            })
    );
    assert!(!request["messages"].to_string().contains(REPLACEMENT));
}

fn note(page: &Page) {
    assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| {
        matches!(part, Part::ToolResult { name, result, .. } if *name == alias("read_note") && result["value"] == "Package-owned note")
    }));
    assert!(page.entries.iter().any(|entry| {
        entry.author == "user"
            && entry
                .parts
                .iter()
                .any(|part| matches!(part, Part::Text(text) if text == PROMPT))
    }));
}

async fn usage(client: &Client, session: &Session, project: ProjectId, responses: u64) {
    use sailry_protocol::usage::{Dimension, Key, Query};
    let now = chrono::Utc::now().timestamp_millis();
    let mut query = Query {
        start_ms: now - 86_400_000,
        end_ms: now + 86_400_000,
        dimension: Dimension::Model,
        projects: vec![],
        worktrees: vec![],
        providers: vec![],
        models: vec![],
        before: None,
    };
    let report = client.read_usage(query.clone()).await.unwrap();
    let owner = Key::Assistant {
        package: PACKAGE.into(),
        id: "notes".into(),
    };
    assert_eq!(report.totals.responses, responses);
    assert_eq!(report.totals.tokens.as_ref().unwrap().input, responses * 12);
    assert_eq!(report.totals.tokens.as_ref().unwrap().output, responses * 4);
    assert!(report.resources.contains(&owner));
    assert!(
        !report
            .resources
            .iter()
            .any(|key| matches!(key, Key::Project(_) | Key::Worktree { .. }))
    );
    assert!(report.requests.items.iter().all(|item| {
        item.session == session.id
            && item.project.is_none()
            && item.worktree == session.worktree
            && item.scope_name == "assistant-notes:notes"
    }));
    for dimension in [Dimension::Project, Dimension::Worktree] {
        query.dimension = dimension;
        let grouped = client.read_usage(query.clone()).await.unwrap();
        assert_eq!(grouped.groups.len(), 1);
        assert_eq!(grouped.groups[0].key, owner);
        assert_eq!(grouped.groups[0].metrics.responses, responses);
    }
    query.worktrees = vec![session.worktree];
    assert_eq!(
        client
            .read_usage(query.clone())
            .await
            .unwrap()
            .totals
            .responses,
        responses
    );
    query.projects = vec![project];
    assert_eq!(client.read_usage(query).await.unwrap().totals.responses, 0);
}

#[tokio::test]
async fn scope() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (alias("read_note"), json!({"key":"review"})),
            (
                plugin_tool("files", "read_file"),
                json!({"path":"source.txt"}),
            ),
            (
                "load_skill".into(),
                json!({"skill":"assistant-notes:review"}),
            ),
            (
                "read_skill_resource".into(),
                json!({"skill":"assistant-notes:review","path":"references/guide.md"}),
            ),
        ])
        .await;
        let fixture = process::Fixture::new(remote, &server).await;
        let info = install(&fixture, 0, false).await;
        let unrelated = unrelated(&fixture).await;
        let session = create(&fixture, &info).await;
        seed(&fixture, &session, &info).await;
        let Output::Snapshot(snapshot) = execute(&fixture.client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let tree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.id == session.worktree)
            .unwrap();
        assert_eq!(tree.project, None);
        assert_ne!(tree.id, fixture.session.worktree);
        assert!(Path::new(&tree.path).starts_with(fixture.node.profile()));
        std::fs::write(
            Path::new(&tree.path).join("source.txt"),
            "Assistant scratch file",
        )
        .unwrap();
        let Output::Session(ordinary) = execute(
            &fixture.client,
            Command::CreateSession {
                project: fixture.session.project,
                worktree: Some(fixture.session.worktree),
                config: Some(fixture.session.config.clone()),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        assert!(ordinary.config.assistant.is_none());
        for assistant in [
            None,
            Some(Binding {
                package: info.summary.reference(),
                id: "database".into(),
            }),
        ] {
            let mut config = session.config.clone();
            config.assistant = assistant;
            let fault = fixture
                .client
                .execute(fixture.client.prepare(Command::SetSessionConfig {
                    session: session.id,
                    expected_revision: session.revision,
                    config,
                }))
                .await
                .unwrap_err();
            assert_eq!(fault.code, ErrorCode::PermissionDenied);
        }
        let mut config = session.config.clone();
        config.assistant.as_mut().unwrap().id = "database".into();
        let fault = fixture
            .client
            .execute(fixture.client.prepare(Command::CreateSession {
                project: None,
                worktree: None,
                config: Some(config),
            }))
            .await
            .unwrap_err();
        assert_eq!(fault.code, ErrorCode::PermissionDenied);
        let session = reject_role(&fixture, &session).await;
        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: unrelated.summary.name,
                expected_revision: unrelated.summary.revision,
                enabled: false,
            },
        )
        .await;
        let turn = submit(&fixture.client, &session, false).await;
        let page = finished(&fixture.client, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        note(&page);
        usage(
            &fixture.client,
            &session,
            fixture.session.project.unwrap(),
            5,
        )
        .await;
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| {
            matches!(part, Part::ToolResult { name, result, .. }
                if name == "load_skill" && result["content"].as_str().is_some_and(|text| text.contains("Read references/guide.md")))
        }));
        assert!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| {
                    matches!(part, Part::ToolResult { name, result, .. }
                if name == "read_skill_resource" && result["content"] == "Review guide")
                })
        );
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| {
            if let Part::ToolResult { name, result, .. } = part && name == &plugin_tool("files", "read_file") {
                matches!(serde_json::from_value::<Output>(result.clone()), Ok(Output::FileContent(content)) if content.text == "Assistant scratch file")
            } else { false }
        }));
        model(&server.requests.lock().unwrap()[0]);
        let Output::Session(saved) = execute(
            &fixture.client,
            Command::ReadSession {
                session: session.id,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        assert_eq!(saved.config.assistant, session.config.assistant);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn recovery() {
    for remote in [false, true] {
        let server = Server::tools(vec![(alias("read_note"), json!({"key":"review"}))]).await;
        let fixture = process::Fixture::new(remote, &server).await;
        let info = install(&fixture, 0, false).await;
        let session = create(&fixture, &info).await;
        seed(&fixture, &session, &info).await;
        let turn = submit(&fixture.client, &session, true).await;
        assert_eq!(turn.config.assistant, session.config.assistant);
        let Output::Plugin(files) = execute(
            &fixture.client,
            Command::ReadPlugin {
                name: "files".into(),
            },
        )
        .await
        else {
            panic!("imported files package expected")
        };
        assert_eq!(
            turn.plugins,
            [info.summary.reference(), files.summary.reference()]
        );
        let replacement = install(&fixture, info.summary.revision, true).await;
        assert_ne!(replacement.summary.digest, info.summary.digest);
        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: PACKAGE.into(),
                expected_revision: replacement.summary.revision,
                enabled: false,
            },
        )
        .await;
        execute(&fixture.client, Command::StartQueuedTurn { turn: turn.id }).await;
        let page = finished(&fixture.client, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        note(&page);
        usage(
            &fixture.client,
            &session,
            fixture.session.project.unwrap(),
            2,
        )
        .await;
        model(&server.requests.lock().unwrap()[0]);
        let fault = fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "New admission".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(fault.code, ErrorCode::NotConfigured);
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start(profile).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let Output::Session(restored) = execute(
            &client,
            Command::ReadSession {
                session: session.id,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        assert_eq!(restored.config.assistant, session.config.assistant);
        assert_eq!(restored.worktree, session.worktree);
        assert_eq!(history(&client, session.id).await, page);
        usage(&client, &restored, fixture.session.project.unwrap(), 2).await;
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
