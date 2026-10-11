use super::*;
use plugin::settings::{Provider, SecretUpdate};
use std::{collections::BTreeMap, sync::atomic::Ordering};
#[path = "../../support/application_tokens.rs"]
mod issuer;

#[tokio::test]
async fn refreshes_persistent_application_transport() {
    for remote in [false, true] {
        let peer = input::Server::url("https://example.invalid/continue", false).await;
        let issuer = issuer::Server::start(Provider::Feishu, Some(&peer.endpoint)).await;
        let model = Server::turn_tools(vec![(alias("input", "read"), json!({}))]).await;
        let fixture = process::Fixture::new(remote, &model).await;
        fixture.node.set_plugin_application_fixture(
            Provider::Feishu,
            &issuer.token_url,
            &issuer.origin,
        );
        package(
            &fixture.root,
            json!({"input":{"type":"streamable-http","url":"https://mcp.feishu.cn/mcp","headers":{"X-Lark-MCP-TAT":""}}}),
            "0.1.0",
        );
        let root = fixture.root.join("package");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(root.join("plugin.json")).unwrap()).unwrap();
        manifest["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1","settings_schema":"dev.sailry.platform/settings.json"}});
        fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
        fs::write(root.join("dev.sailry.platform/settings.json"), json!({
            "$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
            "properties":{"app_id":{"type":"string"},"app_secret":{"type":"string","x-sailry-secret":{
                "server":"input","header":"X-Lark-MCP-TAT","application":{"provider":"feishu","client_id":"app_id"}
            }}},"required":["app_id","app_secret"]
        }).to_string()).unwrap();
        let package = install(&fixture, 0).await;
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        execute(
            &fixture.client,
            Command::SavePluginSettings {
                package: package.summary.reference(),
                values: BTreeMap::from([("app_id".into(), json!("fixture-app"))]),
                secrets: BTreeMap::from([(
                    "app_secret".into(),
                    SecretUpdate::Replace(Secret::new(issuer::SECRET.into())),
                )]),
            },
        )
        .await;
        for index in 0..2 {
            if index > 0 {
                tokio::time::sleep(Duration::from_millis(2100)).await;
            }
            let (_, turn) = submit(&fixture, false).await;
            let (_, pending) = questions::pending(&fixture.client, fixture.session.id).await;
            execute(
                &fixture.client,
                Command::ResolveQuestion {
                    session: fixture.session.id,
                    question: pending.id,
                    response: question::Response::Decline,
                },
            )
            .await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            assert!(
                !serde_json::to_string(&page)
                    .unwrap()
                    .contains(issuer::SECRET)
            );
        }
        assert_eq!(peer.initializations.load(Ordering::SeqCst), 1);
        assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
        assert_eq!(issuer.tokens.load(Ordering::SeqCst), 2);
        let records = issuer.records.lock().unwrap();
        let calls = records
            .iter()
            .filter(|(_, _, body)| body["method"] == "tools/call")
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1["x-lark-mcp-tat"], "application-token-1");
        assert_eq!(calls[1].1["x-lark-mcp-tat"], "application-token-2");
        drop(records);
        assert!(
            !serde_json::to_string(&*model.requests.lock().unwrap())
                .unwrap()
                .contains(issuer::SECRET)
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
