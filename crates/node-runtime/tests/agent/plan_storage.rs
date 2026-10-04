//! Plan restrictions are enforced by the frozen turn, independently of package claims.
use super::*;
use sailry_protocol::plugin::{self, storage::Index, transaction::Operation};
use serde_json::json;

const PACKAGE: &str = "private-plan-fixture";
const SOURCE: &str = r#"
import {getValue,setValue,deleteValue,setConversationValue,deleteConversationValue,indexedValue,prepareTransaction,completeRequest} from 'sailry/sdk';
async function mutations() {
  const writes = [
    () => setValue('hidden',1,'0'),
    () => deleteValue('seed','1'),
    () => indexedValue('indexed',1,{fields:['title','body'],tags:[],order:0},'0'),
    () => prepareTransaction([{kind:'write',data:{key:'bulk',value:1,expected_revision:'0'}}]),
    () => setConversationValue('scoped',1,'0'),
    () => deleteConversationValue('scoped','1'),
    () => prepareTransaction([{kind:'conversation_write',data:{key:'bulk-scoped',value:1,expected_revision:'0'}}])
  ];
  const errors=[];
  for (const write of writes) {
    try { const result=await completeRequest(write()); errors.push(result.Err?.code ?? null); }
    catch (error) { errors.push(error.code); }
  }
  return errors;
}
export async function plain() { return completeRequest(setValue('code','changed','0')); }
export async function declared() { return {read:await getValue('seed'),errors:await mutations()}; }
export async function read() { await mutations(); return {key:'seed'}; }
export async function format({output}) { return {read:output.data,errors:await mutations()}; }
export async function flow({step,outcome}) {
  if (step===0) { await mutations(); return {call:{operation:'storage.get',arguments:{key:'seed'}}}; }
  return {result:{read:outcome.data,errors:await mutations()},output:0};
}
"#;

async fn install(fixture: &process::Fixture) -> plugin::Info {
    let root = fixture.root.join("package");
    std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
    std::fs::write(root.join("dev.sailry.platform/host/main.js"), SOURCE).unwrap();
    std::fs::write(
        root.join("plugin.json"),
        json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":PACKAGE,"version":"1.0.0","extensions":{"dev.sailry.platform":{
                "api_version":"v1","actions":["storage.read","storage.write","conversation.read","conversation.control"],
                "host":{"entry":"dev.sailry.platform/host/main.js",
                    "resources":["dev.sailry.platform/host/main.js"],
                    "handlers":["plain","declared","read","format","flow"]},
                "tools":[
                    {"name":"plain","description":"Write a private value",
                        "handler":{"name":"plain","parameters":{"type":"object"}}},
                    {"name":"declared","description":"Read a private value",
                        "handler":{"name":"declared","read_only":true,"parameters":{"type":"object"}}},
                    {"name":"read","description":"Read a private value",
                        "handler":{"name":"read","result":"format","operation":"storage.get",
                            "parameters":{"type":"object"}}},
                    {"name":"flow","description":"Read through a flow",
                        "handler":{"name":"flow","flow":{"operations":["storage.get"]},
                            "parameters":{"type":"object"}}}
                ]
            }}
        })
        .to_string(),
    )
    .unwrap();
    let Output::Plugin(package) = fixture
        .client
        .execute(fixture.client.prepare(Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "package".into(),
            name: PACKAGE.into(),
            expected_revision: 0,
        }))
        .await
        .unwrap()
    else {
        panic!("plugin expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    package
}

fn scope(
    fixture: &process::Fixture,
    package: &plugin::Info,
    turn: Option<TurnId>,
) -> plugin::Context {
    plugin::Context {
        invocation: None,
        turn,
        surface: Default::default(),
        package: package.summary.reference(),
        worktree: Some(fixture.session.worktree),
        session: Some(fixture.session.id),
    }
}

async fn storage(
    fixture: &process::Fixture,
    scope: &plugin::Context,
    command: Command,
) -> Result<Output, Fault> {
    fixture
        .client
        .execute(fixture.client.prepare(command).with_plugin(scope.clone()))
        .await
}

async fn seed(fixture: &process::Fixture, scope: &plugin::Context) {
    storage(
        fixture,
        scope,
        Command::WritePluginValue {
            key: "seed".into(),
            value: json!("original"),
            expected_revision: 0,
        },
    )
    .await
    .unwrap();
}

async fn value(
    fixture: &process::Fixture,
    scope: &plugin::Context,
    key: &str,
) -> plugin::storage::Entry {
    let Output::PluginValue(value) =
        storage(fixture, scope, Command::ReadPluginValue { key: key.into() })
            .await
            .unwrap()
    else {
        panic!("value expected")
    };
    value
}

async fn queued(fixture: &process::Fixture) -> TurnId {
    let Output::QueuedTurn(turn) = fixture
        .client
        .execute(fixture.client.prepare(Command::QueueTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Exercise private values".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn.id
}

#[tokio::test]
async fn rejects_hidden_writes() {
    for remote in [false, true] {
        for mode in [WorkMode::Plan, WorkMode::Code] {
            let names: &[&str] = if mode == WorkMode::Plan {
                &["declared", "read", "flow"]
            } else {
                &["plain", "declared", "read", "flow"]
            };
            let server = Server::tools(
                names
                    .iter()
                    .map(|name| (plugin_tool(PACKAGE, name), json!({})))
                    .collect(),
            )
            .await;
            let mut fixture = process::Fixture::new(remote, &server).await;
            let package = install(&fixture).await;
            let ui = scope(&fixture, &package, None);
            seed(&fixture, &ui).await;
            planning::configure(&mut fixture, mode).await;
            if mode == WorkMode::Code {
                let mut config = fixture.session.config.clone();
                config.permission = Permission::Ask;
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
            let turn = queued(&fixture).await;
            let live = if mode == WorkMode::Plan {
                WorkMode::Code
            } else {
                WorkMode::Plan
            };
            planning::configure(&mut fixture, live).await;
            fixture
                .client
                .execute(fixture.client.prepare(Command::StartQueuedTurn { turn }))
                .await
                .unwrap();
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(page.runs[0].status, Status::Completed, "{page:?}");
            assert!(page.approvals.is_empty());
            let results: Vec<_> = page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| match part {
                    Part::ToolResult { result, .. } => Some(result),
                    _ => None,
                })
                .collect();
            assert_eq!(results.len(), names.len(), "{page:?}");
            for result in results.iter().skip(usize::from(mode == WorkMode::Code)) {
                assert_eq!(result["read"]["value"], "original", "{result}");
                assert_eq!(result["errors"], json!(vec!["permission_denied"; 7]));
            }
            if mode == WorkMode::Plan {
                for key in ["hidden", "indexed", "bulk", "code"] {
                    let entry = value(&fixture, &ui, key).await;
                    assert!(!entry.present, "{key}");
                    assert_eq!(entry.revision, 0, "{key}");
                }
                assert_eq!(value(&fixture, &ui, "seed").await.revision, 1);
            } else {
                assert_eq!(results[0]["Ok"]["data"]["revision"], "1");
                assert_eq!(value(&fixture, &ui, "code").await.value, "changed");
            }
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), names.len() + 1);
            for request in requests {
                let tools: Vec<_> = request["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|item| item["function"]["name"].as_str())
                    .collect();
                assert_eq!(
                    tools.contains(&plugin_tool(PACKAGE, "plain").as_str()),
                    mode == WorkMode::Code
                );
                for name in ["declared", "read", "flow"] {
                    assert!(tools.contains(&plugin_tool(PACKAGE, name).as_str()));
                }
            }
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn uses_frozen_mode_at_admission() {
    for remote in [false, true] {
        for mode in [WorkMode::Plan, WorkMode::Code] {
            let start = Arc::new(tokio::sync::Notify::new());
            let finish = Arc::new(tokio::sync::Notify::new());
            let server = Server::held(start.clone(), finish.clone()).await;
            let mut fixture = process::Fixture::new(remote, &server).await;
            let package = install(&fixture).await;
            let ui = scope(&fixture, &package, None);
            seed(&fixture, &ui).await;
            planning::configure(&mut fixture, mode).await;
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: "Exercise private values".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let turn = turn.id;
            tokio::time::timeout(Duration::from_secs(10), async {
                while server.requests.lock().unwrap().is_empty() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            let live = if mode == WorkMode::Plan {
                WorkMode::Code
            } else {
                WorkMode::Plan
            };
            planning::configure(&mut fixture, live).await;
            let captured = scope(&fixture, &package, Some(turn));
            assert_eq!(value(&fixture, &captured, "seed").await.value, "original");
            for command in [
                Command::ListPluginKeys {
                    prefix: String::new(),
                    after: None,
                    limit: 10,
                },
                Command::SearchPluginValues(serde_json::from_value(json!({"terms":[]})).unwrap()),
            ] {
                storage(&fixture, &captured, command).await.unwrap();
            }
            let index = Index {
                fields: ["title".into(), "body".into()],
                tags: vec![],
                order: 0,
            };
            let writes = [
                Command::WritePluginValue {
                    key: "direct".into(),
                    value: json!(1),
                    expected_revision: 0,
                },
                Command::WriteIndexedPluginValue {
                    key: "indexed".into(),
                    value: json!(1),
                    index: index.clone(),
                    expected_revision: 0,
                },
                Command::RemovePluginValue {
                    key: "seed".into(),
                    expected_revision: 1,
                },
                Command::WritePluginConversationValue {
                    key: "scoped".into(),
                    value: json!(1),
                    expected_revision: 0,
                },
                Command::RemovePluginConversationValue {
                    key: "scoped".into(),
                    expected_revision: 1,
                },
                Command::PluginTransaction {
                    operations: vec![Operation::ConversationWrite {
                        key: "bulk-scoped".into(),
                        value: json!(1),
                        expected_revision: 0,
                    }],
                },
                Command::PluginTransaction {
                    operations: vec![Operation::ConversationRemove {
                        key: "bulk-scoped".into(),
                        expected_revision: 1,
                    }],
                },
                Command::PluginTransaction {
                    operations: vec![
                        Operation::Write {
                            key: "bulk".into(),
                            value: json!(1),
                            expected_revision: 0,
                        },
                        Operation::Index {
                            key: "bulk-index".into(),
                            value: json!(1),
                            index,
                            expected_revision: 0,
                        },
                        Operation::Remove {
                            key: "seed".into(),
                            expected_revision: if mode == WorkMode::Code { 2 } else { 1 },
                        },
                    ],
                },
            ];
            for command in writes {
                let result = storage(&fixture, &captured, command).await;
                if mode == WorkMode::Plan {
                    assert_eq!(result.unwrap_err().code, ErrorCode::PermissionDenied);
                } else {
                    result.unwrap();
                }
            }
            let original = value(&fixture, &captured, "seed").await;
            assert_eq!(original.present, mode == WorkMode::Plan);
            assert_eq!(
                original.revision,
                if mode == WorkMode::Plan { 1 } else { 3 }
            );
            for key in ["direct", "indexed", "bulk", "bulk-index"] {
                let entry = value(&fixture, &captured, key).await;
                assert_eq!(entry.present, mode == WorkMode::Code, "{key}");
                assert_eq!(
                    entry.revision,
                    if mode == WorkMode::Code { 1 } else { 0 },
                    "{key}"
                );
            }
            for key in ["scoped", "bulk-scoped"] {
                let Output::PluginConversationValue(entry) = storage(
                    &fixture,
                    &captured,
                    Command::ReadPluginConversationValue { key: key.into() },
                )
                .await
                .unwrap() else {
                    panic!("conversation value expected")
                };
                assert!(!entry.present, "{key}");
                assert_eq!(
                    entry.revision,
                    if mode == WorkMode::Code { 2 } else { 0 },
                    "{key}"
                );
            }
            start.notify_one();
            finish.notify_one();
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(page.runs[0].status, Status::Completed);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
