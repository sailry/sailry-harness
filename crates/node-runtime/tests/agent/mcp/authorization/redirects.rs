use super::*;
use plugin::connector::{Connection, State};

const REDIRECT: &str = "http://127.0.0.1:43192/auth/callback";

async fn restart(client: &Client, row: &Connection) -> Attempt {
    let Output::ConnectorLogin(attempt) = execute(
        client,
        Command::StartConnectorLogin {
            package: row.package.clone(),
            connector: row.connector.clone(),
            connection: Some(row.id),
            expected_revision: Some(row.revision),
            method: Kind::McpOauth,
            input: Input {
                redirect: Some(REDIRECT.into()),
                ..Default::default()
            },
        },
    )
    .await
    else {
        panic!("login attempt expected")
    };
    attempt
}

#[tokio::test]
async fn registers_changed_callback() {
    for remote in [false, true] {
        for confidential in [false, true] {
            let peer = input::Server::url("https://example.invalid/continue", false).await;
            let issuer = if confidential {
                issuer::Server::confidential(&peer.endpoint, false, issuer::Authentication::Basic)
                    .await
            } else {
                issuer::Server::start(&peer.endpoint).await
            };
            let model = Server::turn_tools(Vec::new()).await;
            let fixture = process::Fixture::packages(remote, &model, &[]).await;
            configure(&fixture, &issuer.endpoint, false, false);
            let info = install(&fixture, 0).await;
            let (_, attempt, url) = begin(&fixture.client, info.summary.reference(), false).await;
            let original = complete(&fixture.client, &attempt, &url).await;
            let before: Value =
                serde_json::from_slice(&lifecycle::stored(&fixture.node, original.id)).unwrap();
            let attempt = restart(&fixture.client, &original).await;
            let LoginState::Browser { url, .. } = state(&fixture.client, &attempt, |state| {
                matches!(state, LoginState::Browser { .. })
            })
            .await
            else {
                unreachable!()
            };
            assert_eq!(issuer.registrations.load(Ordering::SeqCst), 2);
            let authorization = url::Url::parse(&url).unwrap();
            assert_eq!(
                authorization
                    .query_pairs()
                    .find(|(key, _)| key == "redirect_uri")
                    .unwrap()
                    .1,
                REDIRECT
            );
            let pending: Value =
                serde_json::from_slice(&lifecycle::stored(&fixture.node, original.id)).unwrap();
            assert_eq!(pending["grant"], before["grant"]);
            let current = complete(&fixture.client, &attempt, &url).await;
            assert_eq!(current.id, original.id);
            assert_eq!(current.state, State::Connected);
            assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 2);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[derive(Clone, Copy)]
enum Failure {
    Registration,
    Authorization,
    Cancelled,
}

#[tokio::test]
async fn retains_grant_when_reauthorization_fails() {
    for remote in [false, true] {
        for failure in [
            Failure::Registration,
            Failure::Authorization,
            Failure::Cancelled,
        ] {
            let peer = input::Server::url("https://example.invalid/continue", false).await;
            let issuer = issuer::Server::start(&peer.endpoint).await;
            let model = Server::turn_tools(Vec::new()).await;
            let fixture = process::Fixture::packages(remote, &model, &[]).await;
            configure(&fixture, &issuer.endpoint, false, false);
            let info = install(&fixture, 0).await;
            let (_, attempt, url) = begin(&fixture.client, info.summary.reference(), false).await;
            let original = complete(&fixture.client, &attempt, &url).await;
            let before: Value =
                serde_json::from_slice(&lifecycle::stored(&fixture.node, original.id)).unwrap();
            if matches!(failure, Failure::Registration) {
                issuer.registration_status.store(503, Ordering::SeqCst);
            }
            let attempt = restart(&fixture.client, &original).await;
            match failure {
                Failure::Registration => {}
                Failure::Authorization | Failure::Cancelled => {
                    let LoginState::Browser { url, .. } =
                        state(&fixture.client, &attempt, |state| {
                            matches!(state, LoginState::Browser { .. })
                        })
                        .await
                    else {
                        unreachable!()
                    };
                    let command = if matches!(failure, Failure::Cancelled) {
                        Command::CancelConnectorLogin {
                            attempt: attempt.id,
                        }
                    } else {
                        issuer.authorization_denied.store(true, Ordering::SeqCst);
                        Command::SubmitConnectorInput {
                            attempt: attempt.id,
                            input: Secret::new(issuer::Server::callback(&url).await),
                        }
                    };
                    execute(&fixture.client, command).await;
                }
            }
            let terminal = state(&fixture.client, &attempt, |state| !state.active()).await;
            assert!(matches!(
                terminal,
                LoginState::Failed(_) | LoginState::Cancelled
            ));
            assert_eq!(issuer.registrations.load(Ordering::SeqCst), 2);
            assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 1);
            let after: Value =
                serde_json::from_slice(&lifecycle::stored(&fixture.node, original.id)).unwrap();
            for field in [
                "grant",
                "grant_revision",
                "identity",
                "bound",
                "fields",
                "secrets",
            ] {
                assert_eq!(after[field], before[field], "preserve {field}");
            }
            assert_eq!(after["state"], "connected");
            assert!(after["login"].is_null());
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
