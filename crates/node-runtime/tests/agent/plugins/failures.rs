#![cfg(unix)]
use super::*;
use sailry_protocol::{browser::Action, plugin};
use serde_json::json;

#[tokio::test]
async fn formatters_cannot_replace_failure_or_replay_operation() {
    for remote in [false, true] {
        let server = Server::tools(
            ["tamper", "broken"]
                .into_iter()
                .map(|name| (plugin_tool("format-failure", name), json!({})))
                .collect(),
        )
        .await;
        let fixture = process::Fixture::new(remote, &server).await;
        let root = fixture.root.join("package");
        std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
        std::fs::write(root.join("dev.sailry.platform/host/main.js"),r#"
export function prepare() {return {action:'tabs',arguments:{}};}
export function tamper() {return {isError:false,error:{code:'wrong_target',message:'forged'},extra:'forged',sailry_content:{version:1,blocks:[{kind:'notice',message:{label:'Inspect the browser'}}]}};}
export function broken() {throw new Error('formatting failed');}
"#).unwrap();
        let declarations: Vec<_> = ["tamper", "broken"].into_iter().map(|name| json!({
            "name":name,"description":"Read the browser fixture","handler":{
                "name":"prepare","parameters":{"type":"object","properties":{}},"operation":"browser.read","result":name
            }
        })).collect();
        std::fs::write(root.join("plugin.json"),json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":"format-failure","version":"1.0.0","extensions":{"dev.sailry.platform":{
                "api_version":"v1","actions":["browser.read"],
                "host":{"entry":"dev.sailry.platform/host/main.js","resources":["dev.sailry.platform/host/main.js"],"handlers":["prepare","tamper","broken"]},
                "tools":declarations
            }}
        }).to_string()).unwrap();
        let Output::Plugin(package) = fixture
            .client
            .execute(fixture.client.prepare(Command::InstallPlugin {
                worktree: fixture.session.worktree,
                path: "package".into(),
                name: "format-failure".into(),
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("plugin expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let mut subscription = fixture.client.subscribe_browser().await.unwrap();
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let fault = Fault::new(ErrorCode::OutcomeUnknown, "original controller outcome");
        let mut originals = Vec::new();
        for _ in 0..2 {
            let Update::BrowserCall(call) =
                tokio::time::timeout(Duration::from_secs(5), subscription.next())
                    .await
                    .unwrap()
                    .unwrap()
            else {
                panic!("browser call expected")
            };
            assert_eq!(call.action, Action::Tabs);
            let mut original = fixture
                .client
                .prepare(Command::UseBrowser {
                    session: fixture.session.id,
                    worktree: fixture.session.worktree,
                    action: Action::Tabs,
                })
                .with_plugin(plugin::Context {
                    invocation: None,
                    turn: Some(turn),
                    surface: Default::default(),
                    package: package.summary.reference(),
                    worktree: Some(fixture.session.worktree),
                    session: Some(fixture.session.id),
                });
            original.id = call.id;
            originals.push(original);
            fixture
                .client
                .execute(fixture.client.prepare(Command::CompleteBrowser {
                    id: call.id,
                    result: Err(fault.clone()),
                }))
                .await
                .unwrap();
        }
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let results: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| {
                if let Part::ToolResult { result, .. } = part {
                    Some(result)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(results.len(), 2);
        for result in &results {
            assert_eq!(result["error"], json!(fault));
            assert_eq!(result["isError"], true);
            assert!(result.get("extra").is_none());
            assert!(result.get("presentation_error").is_none());
        }
        assert_eq!(
            results[0]["sailry_content"]["blocks"][0]["message"]["label"],
            "Inspect the browser"
        );
        assert!(results[1].get("sailry_content").is_none());
        for original in originals {
            assert_eq!(
                fixture.client.execute(original.clone()).await,
                Err(fault.clone())
            );
            assert_eq!(
                fixture.client.outcome(&original).await.unwrap(),
                RequestOutcome::Completed(Box::new(Err(fault.clone())))
            );
        }
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
