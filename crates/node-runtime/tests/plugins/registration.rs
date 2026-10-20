//! Bundled registration flows run through the generic private Script runner.
use super::*;
use sailry_protocol::plugin::connector::{Input, Kind, LoginState};
use serde_json::json;
use std::collections::BTreeMap;
#[path = "registration/server.rs"]
mod server;

#[tokio::test]
async fn registers_without_exposing_issued_credentials() {
    for remote in [false, true] {
        let issuer = server::Server::start(vec![]).await;
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        for origin in ["https://accounts.feishu.cn", "https://open.feishu.cn"] {
            node.set_connector_fixture(origin, &issuer.endpoint);
        }
        let bundled: serde_json::Value =
            serde_json::from_str(include_str!("../../../../plugins/feishu/plugin.json")).unwrap();
        let mut declaration = bundled["extensions"]["dev.sailry.platform"]["connectors"][0].clone();
        declaration["inject"] =
            json!([{"origin":"https://open.feishu.cn","header":"X-Access-Token"}]);
        let auth = directory
            .path()
            .join("source/package/dev.sailry.platform/auth");
        fs::create_dir_all(&auth).unwrap();
        fs::write(
            auth.join("registration.js"),
            include_str!("../../../../plugins/feishu/dev.sailry.platform/auth/registration.js"),
        )
        .unwrap();
        let filename = directory.path().join("source/package/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&filename).unwrap()).unwrap();
        manifest["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1","scope":"host","connectors":[declaration]}});
        fs::write(filename, manifest.to_string()).unwrap();
        let package = info(execute(&client, install(worktree, 0)).await);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let Output::ConnectorLogin(attempt) = execute(
            &client,
            Command::StartConnectorLogin {
                package: package.summary.reference(),
                connector: "feishu".into(),
                connection: None,
                expected_revision: None,
                method: Kind::Application,
                input: Input {
                    registration: true,
                    ..Default::default()
                },
            },
        )
        .await
        else {
            panic!("registration attempt expected")
        };
        let mut stream = client.subscribe_connector_login(attempt.id).await.unwrap();
        let url = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let Update::ConnectorLogin(update) = stream.next().await.unwrap() else {
                    panic!("login update expected")
                };
                if let LoginState::Browser { url, .. } = update.state {
                    return url;
                }
                assert!(update.state.active(), "{:?}", update.state);
            }
        })
        .await
        .unwrap();
        let url = reqwest::Url::parse(&url).unwrap();
        let fields: BTreeMap<_, _> = url.query_pairs().collect();
        assert_eq!(fields["createOnly"], "true");
        issuer
            .replies
            .lock()
            .unwrap()
            .push_back(server::Server::success());
        let connected = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let Update::ConnectorLogin(update) = stream.next().await.unwrap() else {
                    panic!("login update expected")
                };
                let public = serde_json::to_string(&update).unwrap();
                assert!(!public.contains("private-app-secret-marker"));
                if let LoginState::Connected(connection) = update.state {
                    return connection;
                }
                assert!(update.state.active(), "{:?}", update.state);
            }
        })
        .await
        .unwrap();
        assert!(connected.configured.contains(&"app_secret".into()));
        assert!(connected.fields.contains_key("app_id"));
        assert_eq!(
            issuer
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, body)| url::form_urlencoded::parse(body.as_bytes())
                    .any(|(name, value)| name == "action" && value == "begin"))
                .count(),
            1
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
