#![cfg(unix)]
use super::*;
use sailry_protocol::{browser::Action, plugin};
use serde_json::{Value, json};

const PACKAGE: &str = "flow-fixture";

async fn install(
    fixture: &process::Fixture,
    source: &str,
    tools: &[(&str, &[&str])],
    message_chars: Option<usize>,
) -> plugin::Info {
    let root = fixture.root.join("package");
    std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
    std::fs::write(root.join("dev.sailry.platform/host/main.js"), source).unwrap();
    let declarations: Vec<_> = tools
        .iter()
        .map(|(name, operations)| {
            json!({
                "name":name,"description":"Exercise a captured operation flow","handler":{
                    "name":name,"parameters":{"type":"object","properties":{}},
                    "flow":{"operations":operations,"message_chars":message_chars}
                }
            })
        })
        .collect();
    std::fs::write(
        root.join("plugin.json"),
        json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":PACKAGE,"version":"1.0.0","extensions":{"dev.sailry.platform":{
                "api_version":"v1","actions":["storage.read","storage.write","browser.read"],
                "host":{"entry":"dev.sailry.platform/host/main.js","resources":["dev.sailry.platform/host/main.js"],
                    "handlers":tools.iter().map(|(name, _)| *name).collect::<Vec<_>>()},
                "tools":declarations
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
        panic!("package expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    package
}

async fn submit(fixture: &process::Fixture, message: String) -> TurnId {
    let Output::QueuedTurn(turn) = fixture
        .client
        .execute(fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: message.into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn.id
}

async fn value(
    fixture: &process::Fixture,
    package: &plugin::Info,
    key: &str,
) -> plugin::storage::Entry {
    let request = fixture
        .client
        .prepare(Command::ReadPluginValue { key: key.into() })
        .with_plugin(plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: Some(fixture.session.worktree),
            session: Some(fixture.session.id),
        });
    let Output::PluginValue(value) = fixture.client.execute(request).await.unwrap() else {
        panic!("value expected")
    };
    value
}

fn results(page: &Page) -> Vec<&Value> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::ToolResult { result, .. } => Some(result),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn approves_once_and_commits_each_step() {
    for remote in [false, true] {
        let arguments = json!({"key":"note","value":"approved"});
        let server =
            Server::tools(vec![(plugin_tool(PACKAGE, "sequence"), arguments.clone())]).await;
        let fixture = process::Fixture::new(remote, &server).await;
        let package = install(&fixture,r#"
export function sequence({arguments:args,step,outcome}) {
  if (step === 0) return {call:{operation:'storage.set',arguments:{key:args.key,value:args.value,expected_revision:0}}};
  if (step === 1) return {call:{operation:'storage.set',arguments:{key:args.key,value:args.value+' twice',expected_revision:outcome.data.revision}}};
  if (step === 2) return {call:{operation:'storage.get',arguments:{key:args.key}}};
  return {result:outcome.data,output:2};
}
"#,&[("sequence",&["storage.set","storage.get"])],None).await;
        let turn = submit(&fixture, "Exercise declared operations".into()).await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert!(!value(&fixture, &package, "note").await.present);
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(page.approvals.len(), 1);
        assert!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part,
            Part::ToolCall { arguments:actual, .. } if actual == &arguments))
        );
        let result = results(&page);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["revision"], 2);
        assert_eq!(result[0]["value"], "approved twice");
        let stored = value(&fixture, &package, "note").await;
        assert_eq!(stored.revision, 2);
        assert_eq!(stored.value, "approved twice");
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_failures_and_completed_effects() {
    for remote in [false, true] {
        let names = ["tamper", "retry", "undeclared", "broken"];
        let server = Server::tools(
            names
                .iter()
                .map(|name| (plugin_tool(PACKAGE, name), json!({})))
                .collect(),
        )
        .await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        let package = install(&fixture,r#"
const write = (key, revision = 0) => ({call:{operation:'storage.set',arguments:{key,value:'changed',expected_revision:revision}}});
export function tamper({step}) {
  if (step === 0) return write('conflict',99);
  return {result:{isError:false,error:null,forged:true,sailry_result:{version:1,diagnostics:[{text:'Recovery remains available',error:false}]}},output:0};
}
export function retry({step}) { return step === 0 ? write('retry-conflict',99) : write('replayed'); }
export function undeclared() { return write('undeclared'); }
export function broken({step}) {
  if (step === 0) return write('completed');
  throw new Error('fixture formatter failed');
}
"#,&[("tamper",&["storage.set"]),("retry",&["storage.set"]),
        ("undeclared",&["storage.get"]),("broken",&["storage.set"])],None).await;
        planning::configure(&mut fixture, WorkMode::Code).await;
        let turn = submit(&fixture, "Exercise declared operations".into()).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let output = results(&page);
        assert_eq!(output.len(), 4, "{page:?}");
        for result in &output[..2] {
            assert_eq!(
                serde_json::from_value::<Fault>(result["error"].clone())
                    .unwrap()
                    .code,
                ErrorCode::RevisionConflict
            );
            assert_eq!(result["isError"], true);
            assert!(result.get("forged").is_none());
        }
        assert_eq!(
            output[0]["sailry_result"]["diagnostics"][0]["text"],
            "Recovery remains available"
        );
        assert_eq!(
            serde_json::from_value::<Fault>(output[2]["error"].clone())
                .unwrap()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(output[3]["revision"], 1);
        assert_eq!(output[3]["value"], "changed");
        // The VM reports its own failure without evaluating a thrown package object.
        assert!(
            output[3]["presentation_error"]
                .as_str()
                .unwrap()
                .contains("plugin JavaScript execution failed"),
            "{}",
            output[3]
        );
        for key in ["conflict", "retry-conflict", "replayed", "undeclared"] {
            assert!(!value(&fixture, &package, key).await.present, "{key}");
        }
        assert_eq!(value(&fixture, &package, "completed").await.revision, 1);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn uncertain_controller_outcomes_never_resume() {
    for remote in [false, true] {
        let server = Server::tools(vec![(plugin_tool(PACKAGE, "uncertain"), json!({}))]).await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        let package = install(&fixture,r#"
export function uncertain({step}) {
  if (step === 0) return {call:{operation:'browser.read',arguments:{action:'tabs',arguments:{}}}};
  if (step === 1) return {call:{operation:'storage.set',arguments:{key:'replayed',value:true,expected_revision:0}}};
  return {result:{forged:true}};
}
"#,&[("uncertain",&["browser.read","storage.set"])],None).await;
        planning::configure(&mut fixture, WorkMode::Code).await;
        let mut subscription = fixture.client.subscribe_browser().await.unwrap();
        let turn = submit(&fixture, "Exercise declared operations".into()).await;
        let Update::BrowserCall(call) =
            tokio::time::timeout(Duration::from_secs(5), subscription.next())
                .await
                .unwrap()
                .unwrap()
        else {
            panic!("browser call expected")
        };
        assert_eq!(call.action, Action::Tabs);
        let fault = Fault::new(ErrorCode::OutcomeUnknown, "original controller outcome");
        fixture
            .client
            .execute(fixture.client.prepare(Command::CompleteBrowser {
                id: call.id,
                result: Err(fault.clone()),
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let output = results(&page);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["error"], json!(fault));
        assert_eq!(output[0]["isError"], true);
        assert!(output[0].get("forged").is_none());
        assert!(!value(&fixture, &package, "replayed").await.present);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), subscription.next())
                .await
                .is_err()
        );
        drop(subscription);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn bounds_the_frozen_message_before_each_continuation() {
    for remote in [false, true] {
        let server = Server::tools(vec![(plugin_tool(PACKAGE, "prefix"), json!({}))]).await;
        let fixture = process::Fixture::new(remote, &server).await;
        install(&fixture,r#"
export function prefix({message,step,state}) {
  if (step === 0) return {call:{operation:'storage.get',arguments:{key:'absent'}},state:{first:message}};
  return {result:{first:state.first,second:message}};
}
"#,&[("prefix",&["storage.get"])],Some(4)).await;
        let escaped_bytes = serde_json::to_vec("\u{0001}").unwrap().len() - 2;
        let message = format!(
            "🙂漢ab{}",
            "\u{0001}".repeat(plugin::host::MAX_DATA_BYTES / escaped_bytes + 1)
        );
        assert!(serde_json::to_vec(&message).unwrap().len() > plugin::host::MAX_DATA_BYTES);
        let turn = submit(&fixture, message).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let output = results(&page);
        assert_eq!(output.len(), 1, "{page:?}");
        assert_eq!(output[0]["first"], "🙂漢ab");
        assert_eq!(output[0]["second"], "🙂漢ab");
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
