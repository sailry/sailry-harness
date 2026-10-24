use super::*;
use sailry_protocol::plugin::connector::State;

#[tokio::test]
async fn expires_unrefreshable_and_invalid_client_grants() {
    for remote in [false, true] {
        for sse in [false, true] {
            for missing in [false, true] {
                let peer = input::Server::url("https://example.invalid/continue", sse).await;
                let issuer =
                    issuer::Server::tokens(&peer.endpoint, !missing, Some("invalid_client")).await;
                let model = Server::turn_tools(vec![(alias("input", "read"), json!({}))]).await;
                let fixture = process::Fixture::packages(remote, &model, &[]).await;
                configure(&fixture, &issuer.endpoint, sse, false);
                let info = install(&fixture, 0).await;
                assert!(info.issues.is_empty(), "{:?}", info.issues);
                let (_, attempt, url) =
                    begin(&fixture.client, info.summary.reference(), false).await;
                let row = complete(&fixture.client, &attempt, &url).await;
                lifecycle::call(&fixture).await;
                assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
                let before: Value =
                    serde_json::from_slice(&lifecycle::stored(&fixture.node, row.id).await)
                        .unwrap();
                let credentials: Value =
                    serde_json::from_str(before["grant"]["access"].as_str().unwrap()).unwrap();
                assert!(credentials["token_response"].is_object());
                assert_eq!(
                    credentials["token_response"]["refresh_token"].as_str(),
                    (!missing).then_some("refresh-initial")
                );
                issuer.status.store(400, Ordering::SeqCst);
                issuer.expire();
                let page = lifecycle::call(&fixture).await;
                assert_eq!(
                    peer.calls.load(Ordering::SeqCst),
                    1,
                    "expired POST must not replay"
                );
                assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 1);
                assert_eq!(
                    issuer.refreshes.load(Ordering::SeqCst),
                    usize::from(!missing)
                );
                let Output::Connections(rows) =
                    execute(&fixture.client, Command::ListConnections).await
                else {
                    panic!("connections expected")
                };
                assert_eq!(rows[0].state, State::Expired, "{remote}/{sse}/{missing}");
                let after: Value =
                    serde_json::from_slice(&lifecycle::stored(&fixture.node, row.id).await)
                        .unwrap();
                assert!(after["grant"].is_null());
                for field in ["fields", "secrets", "revision", "auth_revision"] {
                    assert_eq!(
                        after[field], before[field],
                        "{remote}/{sse}/{missing}/{field}"
                    );
                }
                let public = serde_json::to_string(&(page, rows)).unwrap();
                for secret in ["access-initial", "refresh-initial", "client-private-marker"] {
                    assert!(!public.contains(secret));
                }

                issuer.status.store(200, Ordering::SeqCst);
                let Output::ConnectorLogin(attempt) = execute(
                    &fixture.client,
                    Command::StartConnectorLogin {
                        package: info.summary.reference(),
                        connector: "service".into(),
                        connection: Some(row.id),
                        expected_revision: Some(row.revision),
                        method: Kind::McpOauth,
                        input: Input {
                            redirect: Some("http://127.0.0.1:43191/auth/callback".into()),
                            ..Default::default()
                        },
                    },
                )
                .await
                else {
                    panic!("connector attempt expected")
                };
                let LoginState::Browser { url, .. } = state(&fixture.client, &attempt, |state| {
                    matches!(state, LoginState::Browser { .. })
                })
                .await
                else {
                    unreachable!()
                };
                let restored = complete(&fixture.client, &attempt, &url).await;
                assert_eq!(restored.state, State::Connected);
                lifecycle::recover(&fixture, &peer, 2).await;
                fixture.node.shutdown().await.unwrap();
                fixture.controller.close().await.unwrap();
            }
        }
    }
}
