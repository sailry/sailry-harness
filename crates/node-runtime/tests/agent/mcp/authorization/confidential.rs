use super::*;
#[tokio::test]
async fn restores_confidential_clients_before_refresh() {
    for remote in [false, true] {
        for authentication in [issuer::Authentication::Basic, issuer::Authentication::Post] {
            let peer = input::Server::url("https://example.invalid/continue", false).await;
            let issuer = issuer::Server::confidential(&peer.endpoint, true, authentication).await;
            let model = Server::turn_tools(vec![(alias("input", "read"), json!({}))]).await;
            let fixture = process::Fixture::packages(remote, &model, &[]).await;
            configure(&fixture, &issuer.endpoint, false, true);
            let info = install(&fixture, 0).await;
            assert!(info.issues.is_empty(), "{:?}", info.issues);
            let (_, attempt, url) = begin(&fixture.client, info.summary.reference(), true).await;
            let row = complete(&fixture.client, &attempt, &url).await;
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
            assert_eq!(issuer.registrations.load(Ordering::SeqCst), 0);
            issuer.replace_secret("rotated-client-secret");
            let Output::Connection(changed) = execute(
                &fixture.client,
                Command::UpdateConnection {
                    connection: row.id,
                    expected_revision: row.revision,
                    input: Input {
                        secrets: BTreeMap::from([(
                            "client_secret".into(),
                            Secret::new("rotated-client-secret".into()),
                        )]),
                        ..Default::default()
                    },
                },
            )
            .await
            else {
                panic!("updated connection expected")
            };
            let Output::ConnectorLogin(next) = execute(
                &fixture.client,
                Command::StartConnectorLogin {
                    package: changed.package,
                    connector: "service".into(),
                    connection: Some(row.id),
                    expected_revision: Some(changed.revision),
                    method: Kind::McpOauth,
                    input: Input {
                        redirect: Some("http://127.0.0.1:43191/auth/callback".into()),
                        ..Default::default()
                    },
                },
            )
            .await
            else {
                panic!("login expected")
            };
            let LoginState::Browser { url, .. } = state(&fixture.client, &next, |state| {
                matches!(state, LoginState::Browser { .. })
            })
            .await
            else {
                unreachable!()
            };
            let current = complete(&fixture.client, &next, &url).await;
            assert_eq!(current.id, row.id);
            assert!(current.revision > row.revision);
            assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 2);

            assert!(
                !serde_json::to_string(&page)
                    .unwrap()
                    .contains("client-private-marker")
            );
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
