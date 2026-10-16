use super::*;
use plugin::{Action, Context, http, settings};
use serde_json::json;
use std::{collections::BTreeMap, sync::atomic::Ordering};
#[path = "../support/application_tokens.rs"]
mod issuer;

#[tokio::test]
async fn refreshes_application_http() {
    for remote in [false, true] {
        let server = issuer::Server::start(settings::Provider::Dingtalk, None).await;
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        node.set_plugin_application_fixture(
            settings::Provider::Dingtalk,
            &server.token_url,
            &server.origin,
        );
        let root = directory.path().join("source/package");
        actions::declare(&root, &[Action::Http]);
        let manifest = root.join("plugin.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["extensions"]["dev.sailry.platform"]["settings_schema"] =
            json!("dev.sailry.platform/settings.json");
        fs::write(manifest, value.to_string()).unwrap();
        fs::write(root.join("dev.sailry.platform/settings.json"), json!({
            "$schema":settings::SCHEMA,"type":"object","additionalProperties":false,
            "properties":{"app_id":{"type":"string"},"app_secret":{"type":"string","x-sailry-secret":{
                "origin":"https://api.dingtalk.com","header":"x-acs-dingtalk-access-token","application":{"provider":"dingtalk","client_id":"app_id"}
            }}},"required":["app_id","app_secret"]
        }).to_string()).unwrap();
        let package = info(execute(&client, install(worktree, 0)).await);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let Output::PluginSettings(saved) = execute(
            &client,
            Command::SavePluginSettings {
                package: package.summary.reference(),
                values: BTreeMap::from([("app_id".into(), json!("fixture-app"))]),
                secrets: BTreeMap::from([(
                    "app_secret".into(),
                    settings::SecretUpdate::Replace(Secret::new(issuer::SECRET.into())),
                )]),
            },
        )
        .await
        else {
            panic!("settings expected")
        };
        let context = Context {
            package: saved.package,
            invocation: None,
            turn: None,
            surface: Default::default(),
            worktree: Some(worktree),
            session: None,
        };
        let request = |url: &str| {
            client
                .prepare(Command::RequestPluginHttp(http::Request {
                    url: url.into(),
                    method: "POST".into(),
                    headers: BTreeMap::new(),
                    body: Some("{}".into()),
                    credential: Some("app_secret".into()),
                    timeout_ms: 2000,
                }))
                .with_plugin(context.clone())
        };
        let first = request("https://api.dingtalk.com/probe");
        let output = client.execute(first.clone()).await.unwrap();
        assert!(matches!(&output, Output::PluginHttp(response) if response.status == 200));
        assert_eq!(client.execute(first).await.unwrap(), output);
        assert_eq!(server.tokens.load(Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_millis(2100)).await;
        let (left, right) = tokio::join!(
            client.execute(request("https://api.dingtalk.com/probe")),
            client.execute(request("https://api.dingtalk.com/probe"))
        );
        assert!(left.is_ok(), "{left:?}");
        assert!(right.is_ok(), "{right:?}");
        assert_eq!(server.tokens.load(Ordering::SeqCst), 2);
        let records = server.records.lock().unwrap().clone();
        let resource = records
            .iter()
            .filter(|(path, _, _)| path == "/probe")
            .collect::<Vec<_>>();
        assert_eq!(resource.len(), 3);
        assert_eq!(
            resource[0].1["x-acs-dingtalk-access-token"],
            "application-token-1"
        );
        for record in &resource[1..] {
            assert_eq!(
                record.1["x-acs-dingtalk-access-token"],
                "application-token-2"
            );
        }
        assert_eq!(
            client
                .execute(request("https://example.com/probe"))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        server.failed.store(true, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(2100)).await;
        let failed = request("https://api.dingtalk.com/probe");
        let error = client.execute(failed.clone()).await.unwrap_err();
        assert_eq!(error.message, "application token request failed (HTTP 503)");
        assert_eq!(client.execute(failed).await.unwrap_err(), error);
        assert_eq!(server.tokens.load(Ordering::SeqCst), 3);
        let snapshot = execute(&client, Command::Snapshot).await;
        let public = serde_json::to_string(&(output, error, snapshot)).unwrap();
        assert!(!public.contains(issuer::SECRET));
        assert!(!public.contains("application-token-"));
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
