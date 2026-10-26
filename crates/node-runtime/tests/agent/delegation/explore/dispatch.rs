use super::*;
use sailry_protocol::plugin::storage::Entry;

pub(super) const PACKAGE: &str = "explore-boundary";
pub(super) const EVIDENCE: &str = "Original exploration evidence";
const SOURCE: &str = r#"
import {readFile,getValue,setValue,deleteValue,prepareTransaction,prepareFile,completeRequest} from 'sailry/sdk';
export async function inspect() {
  const attempts = [
    () => prepareFile('changed.txt','Changed',null),
    () => setValue('changed',true,'0'),
    () => deleteValue('retained','0'),
    () => prepareTransaction([{kind:'write',data:{key:'bulk',value:true,expected_revision:'0'}}])
  ];
  const errors = [];
  for (const attempt of attempts) {
    try { const outcome = await completeRequest(attempt()); errors.push(outcome.Err?.code ?? null); }
    catch (error) { errors.push(error.code); }
  }
  return {file:await readFile('source.txt'),value:await getValue('changed'),errors};
}
"#;

pub(super) async fn install(fixture: &Fixture) -> plugin::Info {
    crate::approvals::install(&fixture.client, &["memory"]).await;
    let root = fixture.root().join("boundary");
    std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
    std::fs::write(root.join("dev.sailry.platform/host/main.js"), SOURCE).unwrap();
    std::fs::write(root.join("plugin.json"), json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name":PACKAGE,"version":"1.0.0","extensions":{"dev.sailry.platform":{
            "api_version":"v1","actions":["files.read","files.write","storage.read","storage.write","process.control","browser.read","browser.control"],
            "host":{"entry":"dev.sailry.platform/host/main.js",
                "resources":["dev.sailry.platform/host/main.js"],"handlers":["inspect"]},
            "tools":[{"name":"inspect","description":"Read evidence and verify blocked mutations","handler":{"name":"inspect","read_only":true,
                "parameters":{"type":"object","additionalProperties":false}}}]
        }}
    }).to_string()).unwrap();
    let Output::Plugin(package) = execute(
        &fixture.client,
        Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "boundary".into(),
            name: PACKAGE.into(),
            expected_revision: 0,
            expected_digest: None,
        },
    )
    .await
    else {
        panic!("plugin expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    package
}

pub(super) async fn verify(fixture: &Fixture, child: &Session, package: &plugin::Info) {
    assert_eq!(child.config.mode, WorkMode::Plan);
    let page = history(&fixture.client, child.id).await;
    assert!(page.approvals.is_empty());
    assert!(page.questions.is_empty());
    assert!(!page.runs.is_empty());
    assert!(page.runs.iter().all(|run| run.status == Status::Completed));
    let inspected: Vec<_> = tool_details(&fixture.client, &page)
        .await
        .into_iter()
        .filter(|(name, _)| name == &plugin_tool(PACKAGE, "inspect"))
        .map(|(_, detail)| detail.result.unwrap())
        .collect();
    assert_eq!(inspected.len(), page.runs.len(), "{page:?}");
    for result in inspected {
        assert_eq!(
            result["errors"],
            json!(vec!["permission_denied"; 4]),
            "{result:?}"
        );
        assert!(result["file"].to_string().contains(EVIDENCE));
        assert!(result["value"]["value"].is_null());
    }
    for request in fixture.child.requests.lock().unwrap().iter() {
        let names: Vec<_> = request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&plugin_tool("files", "read_file").as_str()));
        assert!(names.contains(&plugin_tool(PACKAGE, "inspect").as_str()));
        for (package, name) in [
            ("files", "write_file"),
            ("files", "edit_file"),
            ("commands", "run_command"),
            ("memory", "save_memory"),
        ] {
            assert!(!names.contains(&plugin_tool(package, name).as_str()));
        }
    }
    assert_eq!(
        std::fs::read_to_string(fixture.root().join("source.txt")).unwrap(),
        EVIDENCE
    );
    assert!(!fixture.root().join("changed.txt").exists());
    let scope = plugin::Context {
        package: package.summary.reference(),
        surface: Default::default(),
        worktree: Some(child.worktree),
        session: Some(child.id),
        turn: None,
        invocation: None,
    };
    for key in ["changed", "retained", "bulk"] {
        let Output::PluginValue(Entry {
            value,
            present,
            revision,
            ..
        }) = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::ReadPluginValue { key: key.into() })
                    .with_plugin(scope.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("plugin value expected")
        };
        assert!(value.is_null());
        assert!(!present);
        assert_eq!(revision, 0);
    }
}

#[tokio::test]
async fn rejects_captured_mutating_requests() {
    for remote in [false, true] {
        let start = Arc::new(tokio::sync::Notify::new());
        let finish = Arc::new(tokio::sync::Notify::new());
        let child = Server::held(start.clone(), finish.clone()).await;
        let parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"explore","task":"Inspect the existing project","background":true}),
        )])
        .await;
        let mut fixture = Fixture::start(remote, parent, child).await;
        let package = install(&fixture).await;
        std::fs::write(fixture.root().join("source.txt"), EVIDENCE).unwrap();
        select(&mut fixture, true).await;
        let turn = fixture.push("Start an exploration").await;
        fixture.child.wait_count(1).await;
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        let run = history(&fixture.client, child.id).await.runs[0].clone();
        assert_eq!(run.status, Status::Running);
        assert_eq!(child.config.mode, WorkMode::Plan);
        let parent = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            parent
                .runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed,
            "{parent:?}"
        );
        let scope = plugin::Context {
            package: package.summary.reference(),
            surface: Default::default(),
            worktree: Some(child.worktree),
            session: Some(child.id),
            turn: Some(run.turn),
            invocation: None,
        };
        let read = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::ReadFile {
                        worktree: child.worktree,
                        path: "source.txt".into(),
                    })
                    .with_plugin(scope.clone()),
            )
            .await
            .unwrap();
        assert!(
            serde_json::to_value(read)
                .unwrap()
                .to_string()
                .contains(EVIDENCE)
        );
        for command in [
            Command::WriteFile {
                worktree: child.worktree,
                path: "source.txt".into(),
                text: "Changed".into(),
                expected_revision: None,
            },
            Command::RunCommand {
                turn: run.turn,
                command: "touch changed.txt".into(),
                description: None,
                access: sailry_protocol::process::Access::Workspace,
                cwd: String::new(),
                timeout_ms: 1000,
                pty: false,
                background: false,
                attachments: vec![],
            },
            Command::UseBrowser {
                session: child.id,
                worktree: child.worktree,
                action: sailry_protocol::browser::Action::Click {
                    tab: 0,
                    snapshot: "fixture".into(),
                    element: 1,
                },
            },
            Command::WritePluginValue {
                key: "changed".into(),
                value: json!(true),
                expected_revision: 0,
            },
            Command::PluginTransaction {
                operations: vec![plugin::transaction::Operation::Write {
                    key: "bulk".into(),
                    value: json!(true),
                    expected_revision: 0,
                }],
            },
        ] {
            let fault = fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(command.clone())
                        .with_plugin(scope.clone()),
                )
                .await
                .unwrap_err();
            assert_eq!(
                fault.code,
                ErrorCode::PermissionDenied,
                "{command:?}: {fault:?}"
            );
            assert!(
                fault.message.starts_with("planning turns cannot"),
                "{command:?}: {fault:?}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(fixture.root().join("source.txt")).unwrap(),
            EVIDENCE
        );
        assert!(!fixture.root().join("changed.txt").exists());
        let page = history(&fixture.client, child.id).await;
        assert!(page.approvals.is_empty());
        for key in ["changed", "bulk"] {
            let Output::PluginValue(entry) = fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::ReadPluginValue { key: key.into() })
                        .with_plugin(scope.clone()),
                )
                .await
                .unwrap()
            else {
                panic!("plugin value expected")
            };
            assert!(!entry.present);
            assert_eq!(entry.revision, 0);
        }
        start.notify_one();
        finish.notify_one();
        let page = finished(&fixture.client, child.id, run.turn).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let parent = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            parent
                .runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed,
            "{parent:?}"
        );
        let delegated = &results(&fixture.client, &parent).await[0];
        assert_eq!(delegated["admitted"], true);
        assert_eq!(delegated["pending"], true);
        assert_eq!(delegated["status"], "queued");
        assert_eq!(delegated["session"], json!(child.id));
        assert_eq!(delegated["turn"], json!(run.turn));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
