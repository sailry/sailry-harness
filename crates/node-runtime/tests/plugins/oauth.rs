//! OAuth callbacks use the common connection owner on both controller paths.
use super::*;
use plugin::connector::{Attempt, Input, Kind, LoginState};
use serde_json::json;
use std::collections::BTreeMap;
#[path = "oauth/consent.rs"]
mod consent;
#[path = "oauth/lifecycle.rs"]
mod lifecycle;
#[path = "oauth/server.rs"]
mod server;

fn configure(root: &Path) {
    let manifest = root.join("plugin.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1","scope":"host","actions":["http.request"],"connectors":[{
        "id":"account","display":{"label":"Account"},"accounts":"single","origins":["https://issuer.example.com","https://service.example.com"],
        "auth":[{"kind":"oauth2","flow":"pkce","authorize":"https://issuer.example.com/authorize","token":"https://issuer.example.com/token","client":{"fields":{"client_id":"client_id","client_secret":"client_secret"}},"scopes":["read"],"refresh":true}],
        "fields":[{"name":"client_id","title":"Client ID"},{"name":"client_secret","title":"Client secret","secret":true}],
        "inject":[{"origin":"https://service.example.com","header":"Authorization","prefix":"Bearer "}]
    }]}});
    fs::write(manifest, value.to_string()).unwrap();
}
async fn state(
    client: &Client,
    attempt: &Attempt,
    ready: impl Fn(&LoginState) -> bool,
) -> LoginState {
    let mut stream = client.subscribe_connector_login(attempt.id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let Update::ConnectorLogin(update) = stream.next().await.unwrap() else {
                panic!("login update expected")
            };
            let public = serde_json::to_string(&update).unwrap();
            for secret in [
                "private-access-marker",
                "private-refresh-marker",
                "private-client-marker",
            ] {
                assert!(!public.contains(secret));
            }
            if ready(&update.state) {
                return update.state;
            }
            assert!(update.state.active(), "{:?}", update.state);
        }
    })
    .await
    .unwrap()
}
fn callback(url: &str) -> Secret {
    let url = reqwest::Url::parse(url).unwrap();
    let query: BTreeMap<_, _> = url.query_pairs().collect();
    let mut redirect = reqwest::Url::parse(query["redirect_uri"].as_ref()).unwrap();
    redirect
        .query_pairs_mut()
        .append_pair("state", &query["state"])
        .append_pair("code", "single-code");
    Secret::new(redirect.into())
}
#[tokio::test]
async fn scopes_callbacks_and_preserves_receipts() {
    for remote in [false, true] {
        for cancel in [false, true] {
            let server = server::Server::start(false, false).await;
            let (directory, node, controller, client, worktree) = support::empty(remote).await;
            node.set_connector_fixture("https://issuer.example.com", &server.endpoint);
            configure(&directory.path().join("source/package"));
            let package = info(execute(&client, install(worktree, 0)).await);
            assert!(package.issues.is_empty(), "{:?}", package.issues);
            let request = client.prepare(Command::StartConnectorLogin {
                package: package.summary.reference(),
                connector: "account".into(),
                connection: None,
                expected_revision: None,
                method: Kind::Oauth2,
                input: Input {
                    redirect: Some("http://127.0.0.1:43191/auth/callback".into()),
                    fields: BTreeMap::from([("client_id".into(), "registered-client".into())]),
                    secrets: BTreeMap::from([(
                        "client_secret".into(),
                        Secret::new("private-client-marker".into()),
                    )]),
                    ..Default::default()
                },
            });
            let Output::ConnectorLogin(attempt) = client.execute(request.clone()).await.unwrap()
            else {
                panic!("login attempt expected")
            };
            assert_eq!(
                client.execute(request).await.unwrap(),
                Output::ConnectorLogin(attempt.clone())
            );
            let LoginState::Browser { url, .. } = state(&client, &attempt, |s| {
                matches!(s, LoginState::Browser { .. })
            })
            .await
            else {
                unreachable!()
            };
            let other = Client::new(if remote {
                node.local()
            } else {
                controller.handle().remote(node.link().address())
            });
            let denied = match other.subscribe_connector_login(attempt.id).await {
                Ok(mut stream) => stream.next().await.unwrap_err(),
                Err(error) => error,
            };
            assert_eq!(denied.code, ErrorCode::NotFound);
            assert_eq!(
                other
                    .execute(other.prepare(Command::SubmitConnectorInput {
                        attempt: attempt.id,
                        input: callback(&url)
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
            if cancel {
                execute(
                    &client,
                    Command::CancelConnectorLogin {
                        attempt: attempt.id,
                    },
                )
                .await;
                assert_eq!(
                    state(&client, &attempt, |s| matches!(s, LoginState::Cancelled)).await,
                    LoginState::Cancelled
                );
                assert!(server.bodies.lock().unwrap().is_empty());
            } else {
                let completion = client.prepare(Command::SubmitConnectorInput {
                    attempt: attempt.id,
                    input: callback(&url),
                });
                let receipt = client.execute(completion.clone()).await.unwrap();
                assert_eq!(client.execute(completion).await.unwrap(), receipt);
                let LoginState::Connected(connection) =
                    state(&client, &attempt, |s| matches!(s, LoginState::Connected(_))).await
                else {
                    unreachable!()
                };
                assert_eq!(server.bodies.lock().unwrap().len(), 1);
                execute(
                    &client,
                    Command::DisconnectConnection {
                        connection: connection.id,
                        expected_revision: connection.revision,
                    },
                )
                .await;
            }
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}
