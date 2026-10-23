//! MCP authorization shares declared connection records and scoped SDK credentials.
use super::*;
use crate::oauth_server as issuer;
use sailry_protocol::plugin::connector::{Attempt, Input, Kind, LoginState};
use std::{collections::BTreeMap, sync::atomic::Ordering};
mod confidential;
mod expiration;
mod lifecycle;
mod migration;
mod redirects;
mod standalone;
const ORIGIN: &str = "https://mcp.example.com";
fn configure(fixture: &process::Fixture, endpoint: &str, sse: bool, confidential: bool) {
    fixture.node.set_connector_fixture(ORIGIN, endpoint);
    package(
        &fixture.root,
        json!({"input":{"type":if sse{"sse"}else{"streamable-http"},"url":format!("{ORIGIN}/mcp")}}),
        "0.1.0",
    );
    let path = fixture.root.join("package/plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let auth = if confidential {
        json!({"kind":"mcp_oauth","server":"input","client":{"fields":{"client_id":"client_id","client_secret":"client_secret"}}})
    } else {
        json!({"kind":"mcp_oauth","server":"input"})
    };
    manifest["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1","connectors":[{
        "id":"service","display":{"label":"Service"},"accounts":"single","origins":[ORIGIN],"auth":[auth],
        "fields":if confidential{json!([{"name":"client_id","title":"Client ID"},{"name":"client_secret","title":"Client secret","secret":true}])}else{json!([])},
        "inject":[{"server":"input","header":"Authorization","prefix":"Bearer "}]
    }]}});
    fs::write(path, manifest.to_string()).unwrap();
}
async fn state(
    client: &Client,
    attempt: &Attempt,
    ready: impl Fn(&LoginState) -> bool,
) -> LoginState {
    let mut stream = client.subscribe_connector_login(attempt.id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let Update::ConnectorLogin(update) = stream.next().await.unwrap() else {
                panic!("login update expected")
            };
            if ready(&update.state) {
                return update.state;
            }
            assert!(update.state.active(), "{:?}", update.state);
        }
    })
    .await
    .unwrap()
}
async fn begin(
    client: &Client,
    package: plugin::Reference,
    confidential: bool,
) -> (Request, Attempt, String) {
    let request = client.prepare(Command::StartConnectorLogin {
        package,
        connector: "service".into(),
        connection: None,
        expected_revision: None,
        method: Kind::McpOauth,
        input: Input {
            redirect: Some("http://127.0.0.1:43191/auth/callback".into()),
            fields: if confidential {
                BTreeMap::from([("client_id".into(), "fixture-client".into())])
            } else {
                BTreeMap::new()
            },
            secrets: if confidential {
                BTreeMap::from([(
                    "client_secret".into(),
                    Secret::new("client-private-marker".into()),
                )])
            } else {
                BTreeMap::new()
            },
            ..Default::default()
        },
    });
    let Output::ConnectorLogin(attempt) = client.execute(request.clone()).await.unwrap() else {
        panic!("login attempt expected")
    };
    let LoginState::Browser { url, .. } = state(client, &attempt, |state| {
        matches!(state, LoginState::Browser { .. })
    })
    .await
    else {
        unreachable!()
    };
    (request, attempt, url)
}
async fn complete(client: &Client, attempt: &Attempt, url: &str) -> plugin::connector::Connection {
    let request = client.prepare(Command::SubmitConnectorInput {
        attempt: attempt.id,
        input: Secret::new(issuer::Server::callback(url).await),
    });
    let receipt = client.execute(request.clone()).await.unwrap();
    assert_eq!(client.execute(request).await.unwrap(), receipt);
    let LoginState::Connected(row) = state(client, attempt, |state| {
        matches!(state, LoginState::Connected(_))
    })
    .await
    else {
        unreachable!()
    };
    row
}
#[tokio::test]
async fn resumes_authorized_tools() {
    for remote in [false, true] {
        for sse in [false, true] {
            let peer = input::Server::url("https://example.invalid/continue", sse).await;
            let issuer = issuer::Server::start(&peer.endpoint).await;
            let model = Server::turn_tools(vec![(alias("input", "read"), json!({}))]).await;
            let fixture = process::Fixture::packages(remote, &model, &[]).await;
            configure(&fixture, &issuer.endpoint, sse, false);
            let info = install(&fixture, 0).await;
            assert!(info.issues.is_empty(), "{:?}", info.issues);
            let (request, attempt, url) =
                begin(&fixture.client, info.summary.reference(), false).await;
            assert_eq!(
                fixture.client.execute(request).await.unwrap(),
                Output::ConnectorLogin(attempt.clone())
            );
            let other = Client::new(if remote {
                fixture.node.local()
            } else {
                fixture
                    .controller
                    .handle()
                    .remote(fixture.node.link().address())
            });
            let denied = match other.subscribe_connector_login(attempt.id).await {
                Ok(mut stream) => stream.next().await.unwrap_err(),
                Err(error) => error,
            };
            assert_eq!(denied.code, ErrorCode::NotFound);
            let row = complete(&fixture.client, &attempt, &url).await;
            for _ in 0..2 {
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
                let public = serde_json::to_string(&page).unwrap();
                for secret in [
                    "access-private-marker",
                    "refresh-private-marker",
                    "client-private-marker",
                ] {
                    assert!(!public.contains(secret));
                }
            }
            assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
            assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 1);
            issuer.expire();
            let (_, turn) = submit(&fixture, false).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(issuer.refreshes.load(Ordering::SeqCst), 1);
            assert_eq!(
                peer.calls.load(Ordering::SeqCst),
                2,
                "expired POST must not be replayed"
            );
            assert!(
                !serde_json::to_string(&page)
                    .unwrap()
                    .contains("refresh-initial")
            );
            execute(
                &fixture.client,
                Command::DisconnectConnection {
                    connection: row.id,
                    expected_revision: row.revision,
                },
            )
            .await;
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
