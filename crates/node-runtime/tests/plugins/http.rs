use super::*;
use plugin::{Action, Context, http, settings};
use serde_json::json;
use std::collections::BTreeMap;
#[path = "http/server.rs"]
mod server;

fn command(server: &server::Server, path: &str, credential: Option<&str>) -> Command {
    Command::RequestPluginHttp(http::Request {
        url: format!("{}{path}", server.origin),
        method: "POST".into(),
        headers: BTreeMap::new(),
        body: Some("request body".into()),
        credential: credential.map(str::to_owned),
        timeout_ms: 2000,
    })
}

#[tokio::test]
async fn binds_credentials_and_keeps_durable_results() {
    for remote in [false, true] {
        let server = server::Server::start().await;
        let other = server::Server::start().await;
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        actions::declare(&root, &[Action::Http]);
        let manifest = root.join("plugin.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["extensions"]["dev.sailry.platform"]["settings_schema"] =
            json!("dev.sailry.platform/settings.json");
        fs::write(manifest, value.to_string()).unwrap();
        fs::write(
            root.join("dev.sailry.platform/settings.json"),
            json!({
                "$schema":settings::SCHEMA, "type":"object", "additionalProperties":false,
                "properties":{"token":{"type":"string", "x-sailry-secret":{
                    "origin":server.origin, "header":"Authorization", "prefix":"Bearer "
                }}}, "required":["token"]
            })
            .to_string(),
        )
        .unwrap();
        let info = info(execute(&client, install(worktree, 0)).await);
        assert!(info.issues.is_empty(), "{:?}", info.issues);
        assert!(info.mcp.is_empty());
        let Output::PluginSettings(settings) = execute(
            &client,
            Command::SavePluginSettings {
                package: info.summary.reference(),
                values: BTreeMap::new(),
                secrets: BTreeMap::from([(
                    "token".into(),
                    settings::SecretUpdate::Replace(Secret::new("fixture-http-token".into())),
                )]),
            },
        )
        .await
        else {
            panic!("settings expected")
        };
        let context = Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: settings.package,
            worktree: Some(worktree),
            session: None,
        };
        let request = client
            .prepare(command(&server, "/ok", Some("token")))
            .with_plugin(context.clone());
        assert!(
            !serde_json::to_string(&request)
                .unwrap()
                .contains("fixture-http-token")
        );
        let output = client.execute(request.clone()).await.unwrap();
        let Output::PluginHttp(response) = &output else {
            panic!("HTTP response expected")
        };
        assert_eq!(response.status, 201);
        assert_eq!(response.body, "saved");
        assert!(
            !serde_json::to_string(&output)
                .unwrap()
                .contains("fixture-http-token")
        );
        assert_eq!(client.execute(request.clone()).await.unwrap(), output);
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert!(
                requests[0]
                    .0
                    .contains("authorization: Bearer fixture-http-token")
            );
            assert_eq!(requests[0].1, "request body");
        }
        let denied = client
            .prepare(command(&other, "/ok", Some("token")))
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(denied).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        assert!(other.requests.lock().unwrap().is_empty());
        let mut override_header = command(&server, "/ok", Some("token"));
        if let Command::RequestPluginHttp(request) = &mut override_header {
            request
                .headers
                .insert("authorization".into(), "override".into());
        }
        assert_eq!(
            client
                .execute(client.prepare(override_header).with_plugin(context.clone()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            client
                .execute(client.prepare(command(&server, "/ok", None)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        let redirect = client
            .prepare(command(&server, "/redirect", Some("token")))
            .with_plugin(context.clone());
        let Output::PluginHttp(redirect) = client.execute(redirect).await.unwrap() else {
            panic!("HTTP response expected")
        };
        assert_eq!(redirect.status, 302);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        for path in ["/drop", "/large"] {
            let request = client
                .prepare(command(&server, path, None))
                .with_plugin(context.clone());
            let result = client.execute(request.clone()).await.unwrap_err();
            assert_eq!(result.code, ErrorCode::OutcomeUnknown);
            let count = server.requests.lock().unwrap().len();
            assert_eq!(client.execute(request).await.unwrap_err(), result);
            assert_eq!(server.requests.lock().unwrap().len(), count);
        }
        let mut timeout = command(&server, "/hold", None);
        if let Command::RequestPluginHttp(request) = &mut timeout {
            request.timeout_ms = 200;
        }
        let result = client
            .execute(client.prepare(timeout).with_plugin(context.clone()))
            .await
            .unwrap_err();
        assert_eq!(result.code, ErrorCode::OutcomeUnknown);
        assert!(
            server.held.is_cancelled(),
            "the timeout request reached the server"
        );
        tokio::time::timeout(Duration::from_secs(3), server.disconnected.cancelled())
            .await
            .unwrap();
        let count = server.requests.lock().unwrap().len();
        drop(client);
        node.shutdown().await.unwrap();
        let restarted = Node::start(directory.path().join("node")).await.unwrap();
        let retry = Client::new(if remote {
            controller.handle().remote(restarted.link().address())
        } else {
            restarted.local()
        });
        assert_eq!(retry.execute(request).await.unwrap(), output);
        assert_eq!(server.requests.lock().unwrap().len(), count);
        restarted.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
