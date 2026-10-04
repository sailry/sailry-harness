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
async fn selects_tools_without_importing_resources() {
    for remote in [false, true] {
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
            (
                run.clone(),
                json!({"command":"printf imported > imported.txt"}),
            ),
        ])
        .await;
        let fixture = process::Fixture::new(remote, &server).await;
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
        let session = create(&fixture, &owner).await;
        let scratch = scratch(&fixture, &session).await;
        seed(&fixture, &session, &owner).await;
        seed_provider(&fixture, &session, &provider).await;
        let turn = submit(&fixture.client, &session, false).await;
        assert_eq!(turn.plugins.len(), 3);
        assert!(turn.plugins.contains(&provider.summary.reference()));
        let page = finished(&fixture.client, session.id, turn.id).await;
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
}

#[tokio::test]
async fn retains_admitted_handler_and_settings() {
    for remote in [false, true] {
        let tool = plugin_tool(PROVIDER, "inspect");
        let server = Server::turn_tools(vec![(tool.clone(), json!({}))]).await;
        let fixture = process::Fixture::new(remote, &server).await;
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
            let fixture = process::Fixture::new(remote, &server).await;
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
