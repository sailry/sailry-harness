//! Bundled public clients sign in with device codes and rotating refresh tokens.
use super::*;
use sailry_protocol::plugin::connector::State;
use serde_json::Value;
#[path = "device/server.rs"]
mod server;

fn bundled(root: &Path) {
    let manifest = root.join("plugin.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1","scope":"host","actions":["http.request"],"connectors":[{
        "id":"account","display":{"label":"Account"},"accounts":"single","origins":["https://issuer.example.com","https://service.example.com"],
        "auth":[
            {"kind":"oauth2","flow":"device_code","authorize":"https://issuer.example.com/login/oauth/authorize",
                "token":"https://issuer.example.com/login/oauth/access_token","device":"https://issuer.example.com/login/device/code",
                "client":{"bundled":{"client_id":"public-client"}},"scopes":["offline_access"],"refresh":true},
            {"kind":"secret","fields":["token"]}
        ],
        "fields":[{"name":"token","title":"Token","secret":true}],
        "inject":[{"origin":"https://service.example.com","header":"Authorization","prefix":"Bearer "}]
    }]}});
    fs::write(manifest, value.to_string()).unwrap();
}

fn form(body: &str) -> BTreeMap<String, String> {
    url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect()
}

async fn connect(client: &Client, package: plugin::Reference) -> plugin::connector::Connection {
    let Output::ConnectorLogin(attempt) = execute(
        client,
        Command::StartConnectorLogin {
            package,
            connector: "account".into(),
            connection: None,
            expected_revision: None,
            method: Kind::Oauth2,
            input: Input::default(),
        },
    )
    .await
    else {
        panic!("connector login expected")
    };
    let LoginState::Device { code, url, .. } = state(client, &attempt, |state| {
        matches!(state, LoginState::Device { .. })
    })
    .await
    else {
        unreachable!()
    };
    assert_eq!(code, "WDJB-MJHT");
    assert_eq!(url, "https://issuer.example.com/login/device");
    let LoginState::Connected(row) = state(client, &attempt, |state| {
        matches!(state, LoginState::Connected(_))
    })
    .await
    else {
        unreachable!()
    };
    assert_eq!(row.state, State::Connected);
    assert!(row.configured.is_empty());
    row
}

#[tokio::test]
async fn connects_and_rotates_refresh_tokens() {
    let issuer = server::Server::start().await;
    let resource = super::super::http::server::Server::start().await;
    let (directory, node, controller, client, worktree) = support::empty().await;
    node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
    node.set_connector_fixture("https://service.example.com", &resource.origin);
    bundled(&directory.path().join("source/package"));
    let package = info(execute(&client, install(worktree, 0)).await);
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    let row = connect(&client, package.summary.reference()).await;
    {
        let requests = issuer.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[0].0.starts_with("POST /login/device/code "));
        let device = form(&requests[0].1);
        assert_eq!(device["client_id"], "public-client");
        assert_eq!(device["scope"], "offline_access");
        for (head, body) in &requests[1..] {
            assert!(head.starts_with("POST /login/oauth/access_token "));
            let poll = form(body);
            assert_eq!(
                poll["grant_type"],
                "urn:ietf:params:oauth:grant-type:device_code"
            );
            assert_eq!(poll["device_code"], "private-device-marker");
            assert_eq!(poll["client_id"], "public-client");
            assert!(!poll.contains_key("client_secret"));
        }
    }
    let stored: Value = serde_json::from_slice(&lifecycle::stored(&node, row.id).await).unwrap();
    assert_eq!(stored["grant"]["refresh"], "private-refresh-marker-0");
    assert_eq!(stored["secrets"], json!({}));

    let (command, scope) = rejections::request(&row.package, worktree, row.id, false);
    for generation in 1..=2 {
        // Each issued access token expires after one second.
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let output = client
            .execute(client.prepare(command.clone()).with_plugin(scope.clone()))
            .await
            .unwrap();
        assert!(
            matches!(&output, Output::PluginHttp(response) if response.status == 201),
            "{output:?}"
        );
        let refresh = form(&issuer.requests.lock().unwrap().last().unwrap().1);
        assert_eq!(refresh["grant_type"], "refresh_token");
        assert_eq!(
            refresh["refresh_token"],
            format!("private-refresh-marker-{}", generation - 1)
        );
        assert_eq!(refresh["client_id"], "public-client");
        assert!(!refresh.contains_key("client_secret"));
        let stored: Value =
            serde_json::from_slice(&lifecycle::stored(&node, row.id).await).unwrap();
        assert_eq!(
            stored["grant"]["access"],
            format!("private-access-marker-{generation}")
        );
        assert_eq!(
            stored["grant"]["refresh"],
            format!("private-refresh-marker-{generation}")
        );
        let bearer = format!("authorization: Bearer private-access-marker-{generation}");
        let head = resource.requests.lock().unwrap().last().unwrap().0.clone();
        assert!(head.lines().any(|line| line == bearer), "{head}");
    }
    {
        let requests = issuer.requests.lock().unwrap();
        assert_eq!(requests.len(), 5);
        assert!(requests.iter().all(|(head, _)| {
            head.lines()
                .any(|line| line.eq_ignore_ascii_case("accept: application/json"))
        }));
    }
    let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
        panic!("connections expected")
    };
    let public = serde_json::to_string(&rows).unwrap();
    for secret in [
        "private-access-marker",
        "private-refresh-marker",
        "private-device-marker",
    ] {
        assert!(!public.contains(secret));
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn handles_refresh_rejections() {
    for status in [200, 400, 503] {
        let issuer = server::Server::start().await;
        let resource = super::super::http::server::Server::start().await;
        let (directory, node, controller, client, worktree) = support::empty().await;
        node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
        node.set_connector_fixture("https://service.example.com", &resource.origin);
        bundled(&directory.path().join("source/package"));
        let package = info(execute(&client, install(worktree, 0)).await);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let row = connect(&client, package.summary.reference()).await;
        let before: Value =
            serde_json::from_slice(&lifecycle::stored(&node, row.id).await).unwrap();
        *issuer.refresh_error.lock().unwrap() = Some((status, "bad_refresh_token"));
        let (command, scope) = rejections::request(&row.package, worktree, row.id, true);
        let request = client.prepare(command).with_plugin(scope);
        let error = client.execute(request.clone()).await.unwrap_err();
        assert_eq!(
            error.code,
            if status == 503 {
                ErrorCode::OutcomeUnknown
            } else {
                ErrorCode::PermissionDenied
            },
            "{status}"
        );
        let count = resource.requests.lock().unwrap().len();
        assert_eq!(client.execute(request).await.unwrap_err(), error);
        assert_eq!(resource.requests.lock().unwrap().len(), count);
        {
            let requests = issuer.requests.lock().unwrap();
            assert_eq!(requests.len(), 4, "{status}");
            let (head, body) = requests.last().unwrap();
            assert!(head.starts_with("POST /login/oauth/access_token "));
            assert!(
                head.lines()
                    .any(|line| line.eq_ignore_ascii_case("accept: application/json"))
            );
            let refresh = form(body);
            assert_eq!(refresh["refresh_token"], "private-refresh-marker-0");
            assert_eq!(refresh["client_id"], "public-client");
            assert!(!refresh.contains_key("client_secret"));
        }
        let after: Value = serde_json::from_slice(&lifecycle::stored(&node, row.id).await).unwrap();
        if status == 503 {
            assert_eq!(after, before);
        } else {
            assert_eq!(after["state"], "expired");
            assert!(after["grant"].is_null());
            assert_eq!(
                after["grant_revision"].as_u64().unwrap(),
                before["grant_revision"].as_u64().unwrap() + 1
            );
            for field in ["fields", "secrets", "auth_revision", "revision"] {
                assert_eq!(after[field], before[field], "{status}/{field}");
            }
        }
        let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
            panic!("connections expected")
        };
        assert_eq!(
            rows[0].state,
            if status == 503 {
                State::Connected
            } else {
                State::Expired
            },
            "{status}"
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
