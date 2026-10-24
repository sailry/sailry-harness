//! Grant compare-and-swap preserves active accounts around an owned login.
use super::*;
use sailry_protocol::plugin::connector::{Connection, State};
use serde_json::Value;
use std::sync::{Arc, atomic::Ordering};
#[path = "renewals/server.rs"]
mod server;

struct Fixture {
    _directory: tempfile::TempDir,
    node: Node,
    controller: Link,
    client: Arc<Client>,
    worktree: WorktreeId,
    issuer: server::Server,
    resource: super::super::http::server::Server,
    row: Connection,
}
impl Fixture {
    async fn new(remote: bool, health: bool) -> Self {
        let issuer = server::Server::start().await;
        let resource = super::super::http::server::Server::start().await;
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
        node.set_connector_fixture(
            "https://service.example.com",
            if health {
                &issuer.endpoint
            } else {
                &resource.origin
            },
        );
        let root = directory.path().join("source/package");
        configure(&root);
        if health {
            let filename = root.join("plugin.json");
            let mut manifest: Value =
                serde_json::from_slice(&fs::read(&filename).unwrap()).unwrap();
            manifest["extensions"]["dev.sailry.platform"]["connectors"][0]["health"] =
                json!({"request":"https://service.example.com/health","ok":[200]});
            fs::write(filename, manifest.to_string()).unwrap();
        }
        let package = info(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let row = lifecycle::connect(&client, package).await;
        Self {
            _directory: directory,
            node,
            controller,
            client: Arc::new(client),
            worktree,
            issuer,
            resource,
            row,
        }
    }
    fn request(&self, rejected: bool) -> Request {
        let (command, context) =
            rejections::request(&self.row.package, self.worktree, self.row.id, rejected);
        self.client.prepare(command).with_plugin(context)
    }
    async fn stored(&self) -> Value {
        serde_json::from_slice(&lifecycle::stored(&self.node, self.row.id).await).unwrap()
    }
    async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

#[derive(Clone, Copy, Debug)]
enum Finish {
    Cancelled,
    Denied,
    Connected,
}

async fn start(fixture: &Fixture) -> (Attempt, String) {
    let attempt = attempts::start(&fixture.client, &fixture.row, true).await;
    let LoginState::Browser { url, .. } = state(&fixture.client, &attempt, |state| {
        matches!(state, LoginState::Browser { .. })
    })
    .await
    else {
        unreachable!()
    };
    (attempt, url)
}
async fn finish(fixture: &Fixture, attempt: &Attempt, url: &str, outcome: Finish) {
    if matches!(outcome, Finish::Cancelled) {
        execute(
            &fixture.client,
            Command::CancelConnectorLogin {
                attempt: attempt.id,
            },
        )
        .await;
    } else {
        let mut returned = reqwest::Url::parse(callback(url).expose()).unwrap();
        let state = returned
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .into_owned();
        returned.set_query(None);
        {
            let mut query = returned.query_pairs_mut();
            query.append_pair("state", &state);
            if matches!(outcome, Finish::Denied) {
                query.append_pair("error", "access_denied");
            } else {
                query.append_pair("code", "account-b-code");
            }
        }
        execute(
            &fixture.client,
            Command::SubmitConnectorInput {
                attempt: attempt.id,
                input: Secret::new(returned.into()),
            },
        )
        .await;
    }
    let result = state(&fixture.client, attempt, |state| !state.active()).await;
    assert!(
        match (outcome, result) {
            (Finish::Cancelled, LoginState::Cancelled)
            | (Finish::Connected, LoginState::Connected(_)) => true,
            (Finish::Denied, LoginState::Failed(error)) =>
                error.code == ErrorCode::PermissionDenied,
            _ => false,
        },
        "{outcome:?}"
    );
}

#[tokio::test]
async fn preserves_renewals_around_pending_sign_in() {
    for remote in [false, true] {
        for before in [false, true] {
            for outcome in [Finish::Cancelled, Finish::Denied, Finish::Connected] {
                let fixture = Fixture::new(remote, false).await;
                let original = fixture.stored().await;
                let (entered, release) = fixture.issuer.hold();
                let pending = if before {
                    None
                } else {
                    Some(start(&fixture).await)
                };
                let client = fixture.client.clone();
                let request = fixture.request(true);
                let refreshing = tokio::spawn(async move { client.execute(request).await });
                tokio::time::timeout(Duration::from_secs(5), entered)
                    .await
                    .unwrap()
                    .unwrap();
                let (attempt, url) = if let Some(pending) = pending {
                    pending
                } else {
                    start(&fixture).await
                };
                release.send(()).unwrap();
                let result = refreshing.await.unwrap().unwrap();
                assert!(
                    matches!(result, Output::PluginHttp(response) if response.status == 401),
                    "{remote}/{before}/{outcome:?}"
                );
                let renewed = fixture.stored().await;
                assert_eq!(renewed["state"], "connecting");
                assert_eq!(renewed["grant"]["access"], "account-a-renewed-1");
                assert_eq!(
                    renewed["grant_revision"].as_u64().unwrap(),
                    original["grant_revision"].as_u64().unwrap() + 1
                );
                finish(&fixture, &attempt, &url, outcome).await;
                let final_record = fixture.stored().await;
                assert_eq!(final_record["state"], "connected");
                assert!(final_record["login"].is_null());
                assert_eq!(final_record["revision"], fixture.row.revision + 1);
                let connected = matches!(outcome, Finish::Connected);
                let token = if connected {
                    "account-b-access"
                } else {
                    "account-a-renewed-1"
                };
                assert_eq!(final_record["grant"]["access"], token);
                assert_eq!(
                    final_record["grant_revision"].as_u64().unwrap(),
                    renewed["grant_revision"].as_u64().unwrap() + u64::from(connected)
                );
                assert!(fixture.client.execute(fixture.request(false)).await.is_ok());
                {
                    let requests = fixture.resource.requests.lock().unwrap();
                    assert_eq!(
                        requests
                            .iter()
                            .filter(|(head, _)| head.starts_with("POST /unauthorized "))
                            .count(),
                        1
                    );
                    assert!(
                        requests
                            .last()
                            .unwrap()
                            .0
                            .contains(&format!("authorization: Bearer {token}\r\n"))
                    );
                }
                let Output::Connections(rows) =
                    execute(&fixture.client, Command::ListConnections).await
                else {
                    panic!("connections expected")
                };
                let public = serde_json::to_string(&rows).unwrap();
                for secret in [
                    "account-a-access",
                    "account-a-renewed",
                    "account-b-access",
                    "account-a-refresh",
                    "account-b-refresh",
                    "private-client-marker",
                ] {
                    assert!(!public.contains(secret));
                }
                fixture.close().await;
            }
        }
    }
}

#[tokio::test]
async fn rejects_late_renewals_after_replacing_accounts() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, false).await;
        let (entered, release) = fixture.issuer.hold();
        let client = fixture.client.clone();
        let request = fixture.request(true);
        let refreshing = tokio::spawn(async move { client.execute(request).await });
        tokio::time::timeout(Duration::from_secs(5), entered)
            .await
            .unwrap()
            .unwrap();
        let (attempt, url) = start(&fixture).await;
        finish(&fixture, &attempt, &url, Finish::Connected).await;
        let before = lifecycle::stored(&fixture.node, fixture.row.id).await;
        release.send(()).unwrap();
        let error = refreshing.await.unwrap().unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        assert_eq!(
            lifecycle::stored(&fixture.node, fixture.row.id).await,
            before
        );
        assert_eq!(
            fixture.stored().await["grant"]["access"],
            "account-b-access"
        );
        assert!(fixture.client.execute(fixture.request(false)).await.is_ok());
        assert!(
            fixture
                .resource
                .requests
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .0
                .contains("authorization: Bearer account-b-access\r\n")
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn renews_before_rechecking_health() {
    for remote in [false, true] {
        for denied in [false, true] {
            let fixture = Fixture::new(remote, true).await;
            let before = fixture.stored().await;
            fixture.issuer.requests.lock().unwrap().clear();
            *fixture.issuer.expected.lock().unwrap() = "rejected-access".into();
            fixture.issuer.denied.store(denied, Ordering::SeqCst);
            let result = fixture
                .client
                .execute(fixture.client.prepare(Command::ProbeConnection {
                    connection: fixture.row.id,
                }))
                .await;
            if denied {
                assert_eq!(result.unwrap_err().code, ErrorCode::Expired);
            } else {
                assert!(
                    matches!(result.unwrap(), Output::Connection(row) if row.state == State::Connected)
                );
            }
            let current = fixture.stored().await;
            assert_eq!(
                current["grant_revision"].as_u64().unwrap(),
                before["grant_revision"].as_u64().unwrap() + 1 + u64::from(denied)
            );
            assert_eq!(current["grant"].is_null(), denied);
            assert_eq!(
                current["state"],
                if denied { "expired" } else { "connected" }
            );
            {
                let requests = fixture.issuer.requests.lock().unwrap();
                assert_eq!(
                    requests
                        .iter()
                        .filter(|(head, _)| head.starts_with("GET /health "))
                        .count(),
                    2
                );
                let refresh = requests
                    .iter()
                    .filter(|(head, _)| head.starts_with("POST /token "))
                    .collect::<Vec<_>>();
                assert_eq!(refresh.len(), 1);
                let fields: BTreeMap<_, _> = url::form_urlencoded::parse(refresh[0].1.as_bytes())
                    .into_owned()
                    .collect();
                assert_eq!(fields["grant_type"], "refresh_token");
                assert_eq!(fields["refresh_token"], "account-a-refresh");
                assert_eq!(fields["client_secret"], "private-client-marker");
            }
            fixture.close().await;
        }
    }
}
