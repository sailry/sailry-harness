use super::*;
use std::collections::BTreeMap;

const PROVIDER: &str = "assistant-tools";

async fn provider(fixture: &process::Fixture, revision: u64, version: &str) -> plugin::Info {
    let root = fixture.root.join("provider");
    std::fs::create_dir_all(root.join("skills/hidden")).unwrap();
    std::fs::create_dir_all(root.join(plugin::NAMESPACE)).unwrap();
    std::fs::write(root.join("skills/hidden/SKILL.md"),
        "---\nname: hidden\ndescription: Imported package skill must remain unavailable\n---\nHidden provider instructions\n").unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/main.js"),
        format!(
            r#"
import {{ context, readSettings, readFile, getValue }} from 'sailry/sdk';
export async function inspect() {{
  const scope = context();
  const settings = await readSettings();
  const file = await readFile('source.txt');
  const note = await getValue('review');
  return {{version:{version:?}, package:scope.package.name, session:scope.session,
    worktree:scope.worktree, label:settings.values.label, text:file.text, note:note.value}};
}}
export function hidden() {{ throw new Error('Unselected tool executed'); }}
"#
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/settings.json"),
        json!({
            "$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
            "properties":{"label":{"type":"string","default":"Default"}},"required":["label"]
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(root.join("mcp.json"), json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers":{"hidden":{"type":"stdio","command":"unselected-assistant-provider-server"}}
    }).to_string()).unwrap();
    std::fs::write(root.join("plugin.json"), json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name":PROVIDER,"version":version,"extensions":{"dev.sailry.platform":{
            "api_version":"v1","actions":["files.read","storage.read","storage.write"],
            "settings_schema":"dev.sailry.platform/settings.json",
            "host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],"handlers":["inspect","hidden"]},
            "tools":[
                {"name":"inspect","description":"Read the assistant scratch file",
                    "handler":{"name":"inspect","parameters":{"type":"object","properties":{}}}},
                {"name":"read_note","operation":"storage.get"},
                {"name":"hidden","description":"An unselected tool",
                    "handler":{"name":"hidden","parameters":{"type":"object","properties":{}}}},
                {"name":"list_notes","operation":"storage.list"},
                {"name":"hidden_mcp","server":"hidden"}
            ]
        }}
    }).to_string()).unwrap();
    let Output::Plugin(info) = execute(
        &fixture.client,
        Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "provider".into(),
            name: PROVIDER.into(),
            expected_revision: revision,
            expected_digest: None,
        },
    )
    .await
    else {
        panic!("package expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

async fn configure(fixture: &process::Fixture, info: &plugin::Info, label: &str) -> plugin::Info {
    execute(
        &fixture.client,
        Command::SavePluginSettings {
            package: info.summary.reference(),
            values: BTreeMap::from([("label".into(), json!(label))]),
            secrets: BTreeMap::new(),
        },
    )
    .await;
    let Output::Plugin(info) = execute(
        &fixture.client,
        Command::ReadPlugin {
            name: PROVIDER.into(),
        },
    )
    .await
    else {
        panic!("package expected")
    };
    info
}

async fn assistant(fixture: &process::Fixture, tools: Value) -> plugin::Info {
    let info = install(fixture, 0, false).await;
    let path = fixture.root.join("assistant/plugin.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["extensions"]["dev.sailry.platform"]["desktop"]["conversations"][0]["tools"] = tools;
    std::fs::write(path, manifest.to_string()).unwrap();
    load(fixture, info.summary.revision).await
}

async fn scratch(fixture: &process::Fixture, session: &Session) -> std::path::PathBuf {
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
    let path = std::path::PathBuf::from(&tree.path);
    assert!(path.starts_with(fixture.node.profile()));
    std::fs::write(path.join("source.txt"), "Assistant scratch file").unwrap();
    path
}

async fn seed_provider(fixture: &process::Fixture, session: &Session, provider: &plugin::Info) {
    let request = fixture
        .client
        .prepare(Command::WritePluginValue {
            key: "review".into(),
            value: json!("Provider-owned note"),
            expected_revision: 0,
        })
        .with_plugin(plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: provider.summary.reference(),
            worktree: Some(session.worktree),
            session: Some(session.id),
        });
    assert!(matches!(
        fixture.client.execute(request).await.unwrap(),
        Output::PluginValue(_)
    ));
}

fn result<'a>(page: &'a Page, tool: &str) -> &'a Value {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .find_map(|part| match part {
            Part::ToolResult { name, result, .. } if name == tool => Some(result),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing result {tool}: {page:?}"))
}

#[tokio::test]
async fn qualifies_policy_without_a_loader() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        let info = assistant(&fixture, json!([{"kind":"plugin","name":"read_note"}])).await;
        let session = create(&fixture, &info).await;
        let turn = submit(&fixture.client, &session, false).await;
        let page = finished(&fixture.client, session.id, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed
        );
        let request = server.requests.lock().unwrap()[0].clone();
        let tools: std::collections::BTreeSet<_> = request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        assert!(tools.contains(alias("read_note").as_str()));
        assert!(!tools.contains("load_skill"));
        assert!(!tools.contains("read_skill_resource"));
        let instruction = request["messages"][0].to_string();
        assert!(instruction.contains("assistant-notes:review"));
        assert!(instruction.contains("When load_skill is available in this turn's tool catalog"));
        assert!(
            instruction
                .contains("If a required skill or tool is unavailable, explain the limitation")
        );
        assert!(
            instruction.contains(
                "unless the full instructions are already supplied; do not load them again"
            )
        );
        assert!(!instruction.contains("delegation:compare-worktrees"));
        assert!(!instruction.contains("<selected_skill>"));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn selects_tools_without_importing_resources() {
    for remote in [false, true] {
        selected(remote, false, Permission::Full).await;
    }
}

#[tokio::test]
async fn requires_unrestricted_consent() {
    for remote in [false, true] {
        for permission in [Permission::Ask, Permission::Project] {
            selected(remote, true, permission).await;
        }
    }
}

#[tokio::test]
async fn executes_unrestricted_calls_in_full_mode() {
    for remote in [false, true] {
        selected(remote, true, Permission::Full).await;
    }
}

async fn files(fixture: &process::Fixture, session: &Session, root: &Path) {
    let Output::FileWritten(written) = execute(
        &fixture.client,
        Command::WriteFile {
            worktree: session.worktree,
            path: "native.txt".into(),
            text: "Original note".into(),
            expected_revision: None,
        },
    )
    .await
    else {
        panic!("written file expected")
    };
    let Output::FileWritten(edited) = execute(
        &fixture.client,
        Command::EditFile {
            worktree: session.worktree,
            path: written.path.clone(),
            old_text: Some("Original".into()),
            new_text: Some("Updated".into()),
            replace_all: false,
            edits: None,
            expected_revision: written.revision.clone(),
        },
    )
    .await
    else {
        panic!("edited file expected")
    };
    assert_eq!(
        std::fs::read_to_string(root.join("native.txt")).unwrap(),
        "Updated note"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("source.txt")).unwrap(),
        "Assistant scratch file"
    );
    assert!(!fixture.root.join("native.txt").exists());
    let stale = fixture
        .client
        .execute(fixture.client.prepare(Command::EditFile {
            worktree: session.worktree,
            path: written.path,
            old_text: Some("Updated".into()),
            new_text: Some("Stale".into()),
            replace_all: false,
            edits: None,
            expected_revision: written.revision,
        }))
        .await
        .unwrap_err();
    assert_eq!(stale.code, ErrorCode::RevisionConflict);
    let Output::FileContent(file) = execute(
        &fixture.client,
        Command::ReadFile {
            worktree: session.worktree,
            path: "native.txt".into(),
        },
    )
    .await
    else {
        panic!("file expected")
    };
    assert_eq!(file.revision, Some(edited.revision));
    assert_eq!(file.text, "Updated note");
}

async fn selected(remote: bool, unrestricted: bool, permission: Permission) {
    let command = if unrestricted {
        json!({"command":"printf imported >> imported.txt", "access":"unrestricted"})
    } else {
        json!({"command":"printf imported >> imported.txt"})
    };
    let inspect = plugin_tool(PROVIDER, "inspect");
    let read = plugin_tool(PROVIDER, "read_note");
    let run = plugin_tool("commands", "run_command");
    let server = Server::tools(vec![
        (inspect.clone(), json!({})),
        (read.clone(), json!({"key":"review"})),
        (alias("read_note"), json!({"key":"review"})),
        (
            "load_skill".into(),
            json!({"skill":"assistant-tools:hidden"}),
        ),
        (
            "load_skill".into(),
            json!({"skill":"assistant-notes:review"}),
        ),
        (run.clone(), command),
    ])
    .await;
    let fixture = process::Fixture::packages(remote, &server, &["commands"]).await;
    let provider = provider(&fixture, 0, "1.0.0").await;
    let provider = configure(&fixture, &provider, "Captured label").await;
    let owner = assistant(
        &fixture,
        json!([
            {"kind":"package","package":PROVIDER,"name":"inspect"},
            {"kind":"package","package":PROVIDER,"name":"read_note"},
            {"kind":"package","package":"commands","name":"run_command"},
            {"kind":"plugin","name":"read_note"},
            {"kind":"builtin","name":"load_skill"},
            {"kind":"builtin","name":"read_skill_resource"}
        ]),
    )
    .await;
    let mut session = create(&fixture, &owner).await;
    if session.config.permission != permission {
        let mut config = session.config.clone();
        config.permission = permission;
        let Output::Session(updated) = execute(
            &fixture.client,
            Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        session = updated;
    }
    let scratch = scratch(&fixture, &session).await;
    if !unrestricted {
        files(&fixture, &session, &scratch).await;
    }
    seed(&fixture, &session, &owner).await;
    seed_provider(&fixture, &session, &provider).await;
    let turn = submit(&fixture.client, &session, false).await;
    assert_eq!(turn.plugins.len(), 3);
    assert!(turn.plugins.contains(&provider.summary.reference()));
    if unrestricted && permission != Permission::Full {
        let (_, approval) = approvals::pending(&fixture.client, session.id).await;
        assert_eq!(approval.state, ApprovalState::Pending);
        assert_eq!(approval.source, ApprovalSource::User);
        assert_eq!(
            approval.access,
            Some(sailry_protocol::process::Access::Unrestricted)
        );
        assert!(!approval.can_remember);
        assert_eq!(approval.choice, None);
        assert_eq!(approval.prefix, None);
        assert!(!scratch.join("imported.txt").exists());
        process::decide(&fixture.client, &approval, Decision::Approve).await;
    }
    let page = finished(&fixture.client, session.id, turn.id).await;
    if unrestricted {
        assert_eq!(page.approvals.len(), 1);
        let approval = &page.approvals[0];
        assert_eq!(approval.state, ApprovalState::Approved);
        assert_eq!(
            approval.access,
            Some(sailry_protocol::process::Access::Unrestricted)
        );
        assert!(!approval.can_remember);
        assert_eq!(approval.prefix, None);
        assert!(approvals::rules::claimed(&fixture.node, approval).1);
        if permission == Permission::Full {
            assert_eq!(approval.source, ApprovalSource::Full);
            assert_eq!(approval.choice, None);
        } else {
            assert_eq!(approval.source, ApprovalSource::User);
            assert_eq!(approval.choice, Some(ApprovalChoice::Once));
        }
    }
    if !unrestricted {
        assert!(
            page.approvals
                .iter()
                .all(|approval| approval.source != ApprovalSource::User)
        );
    }
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{page:?}"
    );
    assert_eq!(
        result(&page, &inspect),
        &json!({
            "version":"1.0.0","package":PROVIDER,"session":session.id,"worktree":session.worktree,
            "label":"Captured label","text":"Assistant scratch file","note":"Provider-owned note"
        })
    );
    assert_eq!(result(&page, &read)["value"], "Provider-owned note");
    assert_eq!(result(&page, &run)["kind"], "command_result");
    assert_eq!(
        result(&page, &run)["data"]["outcome"],
        json!({"kind":"exited","data":0})
    );
    note(&page);
    assert!(result(&page, "load_skill")["error"].is_object());
    assert_eq!(
        std::fs::read_to_string(scratch.join("imported.txt")).unwrap(),
        "imported"
    );
    assert!(!fixture.root.join("imported.txt").exists());
    let request = server.requests.lock().unwrap()[0].clone();
    let names: BTreeSet<_> = request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        names,
        BTreeSet::from([
            inspect,
            read,
            run,
            alias("read_note"),
            "load_skill".into(),
            "read_skill_resource".into(),
            "ask_user".into(),
            "set_session_title".into(),
            "compact_context".into()
        ])
    );
    let messages = request["messages"].to_string();
    assert!(messages.contains("assistant-notes:review"));
    assert!(!messages.contains("assistant-tools:hidden"));
    assert!(!messages.contains("MCP server assistant-tools/hidden"));
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn retains_admitted_handler_and_settings() {
    for remote in [false, true] {
        let tool = plugin_tool(PROVIDER, "inspect");
        let server = Server::turn_tools(vec![(tool.clone(), json!({}))]).await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        let first = provider(&fixture, 0, "1.0.0").await;
        let first = configure(&fixture, &first, "First settings").await;
        let owner = assistant(
            &fixture,
            json!([
                {"kind":"package","package":PROVIDER,"name":"inspect"}
            ]),
        )
        .await;
        let session = create(&fixture, &owner).await;
        scratch(&fixture, &session).await;
        seed_provider(&fixture, &session, &first).await;
        let original = submit(&fixture.client, &session, true).await;
        assert_eq!(
            original.plugins,
            [owner.summary.reference(), first.summary.reference()]
        );
        let second = provider(&fixture, first.summary.revision, "2.0.0").await;
        let second = configure(&fixture, &second, "Second settings").await;
        let current = submit(&fixture.client, &session, true).await;
        assert_eq!(
            current.plugins,
            [owner.summary.reference(), second.summary.reference()]
        );
        execute(
            &fixture.client,
            Command::RemovePlugin {
                name: PROVIDER.into(),
                expected_revision: second.summary.revision,
            },
        )
        .await;
        for (turn, version, label) in [
            (original, "1.0.0", "First settings"),
            (current, "2.0.0", "Second settings"),
        ] {
            execute(&fixture.client, Command::StartQueuedTurn { turn: turn.id }).await;
            let page = finished(&fixture.client, session.id, turn.id).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                Status::Completed,
                "{page:?}"
            );
            let results: Vec<_> = page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| match part {
                    Part::ToolResult { name, result, .. } if name == &tool => Some(result),
                    _ => None,
                })
                .collect();
            let result = results.last().unwrap();
            assert_eq!(result["version"], version, "{result:?}");
            assert_eq!(result["label"], label, "{result:?}");
            assert_eq!(result["text"], "Assistant scratch file");
            assert_eq!(result["note"], "Provider-owned note");
        }
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
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn requires_an_available_declared_tool() {
    for remote in [false, true] {
        for case in ["missing", "disabled", "unknown_tool", "mcp"] {
            let server = Server::tools(vec![]).await;
            let fixture = process::Fixture::packages(remote, &server, &[]).await;
            let package = provider(&fixture, 0, "1.0.0").await;
            if case == "disabled" {
                execute(
                    &fixture.client,
                    Command::SetPluginEnabled {
                        name: PROVIDER.into(),
                        expected_revision: package.summary.revision,
                        enabled: false,
                    },
                )
                .await;
            }
            let owner = assistant(&fixture,json!([{
                "kind":"package","package":if case == "missing" {"missing-tools"} else {PROVIDER},
                "name":match case {"unknown_tool"=>"missing", "mcp"=>"hidden_mcp",_=>"inspect"}
            }])).await;
            let session = create(&fixture, &owner).await;
            let fault = fixture
                .client
                .execute(fixture.client.prepare(Command::QueueTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: "Rejected admission".into(),
                }))
                .await
                .unwrap_err();
            assert_eq!(fault.code, ErrorCode::NotConfigured, "{case}: {fault:?}");
            assert!(server.requests.lock().unwrap().is_empty());
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
