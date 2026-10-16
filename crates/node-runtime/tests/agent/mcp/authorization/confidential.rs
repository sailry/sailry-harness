use super::*;
use plugin::settings::SecretUpdate;
use std::collections::BTreeMap;

#[tokio::test]
async fn restores_confidential_clients_before_refresh() {
    for remote in [false, true] {
        for (registered, authentication) in [
            (false, issuer::Authentication::Basic),
            (false, issuer::Authentication::Post),
            (true, issuer::Authentication::Post),
        ] {
            let peer = input::Server::url("https://example.invalid/continue", false).await;
            let issuer =
                issuer::Server::confidential(&peer.endpoint, registered, authentication).await;
            let model = Server::turn_tools(vec![(alias("input", "read"), json!({}))]).await;
            let fixture = process::Fixture::packages(remote, &model, &[]).await;
            package(
                &fixture.root,
                json!({"input":{"type":"streamable-http","url":issuer.endpoint}}),
                "0.1.0",
            );
            if registered {
                let root = fixture.root.join("package");
                let mut manifest: Value =
                    serde_json::from_slice(&fs::read(root.join("plugin.json")).unwrap()).unwrap();
                manifest["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1","settings_schema":"dev.sailry.platform/settings.json"}});
                fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
                fs::write(root.join("dev.sailry.platform/settings.json"), json!({
                    "$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
                    "properties":{"client_id":{"type":"string"},"client_secret":{"type":"string","x-sailry-secret":{"server":"input","oauth":{"client_id":"client_id"}}}},
                    "required":["client_id","client_secret"]
                }).to_string()).unwrap();
            }
            let package = install(&fixture, 0).await;
            assert!(package.issues.is_empty(), "{:?}", package.issues);
            let reference = if registered {
                let Output::PluginSettings(settings) = execute(
                    &fixture.client,
                    Command::SavePluginSettings {
                        package: package.summary.reference(),
                        values: BTreeMap::from([("client_id".into(), json!("fixture-client"))]),
                        secrets: BTreeMap::from([(
                            "client_secret".into(),
                            SecretUpdate::Replace(Secret::new("client-private-marker".into())),
                        )]),
                    },
                )
                .await
                else {
                    panic!("settings expected")
                };
                let denied = fixture.client.prepare(Command::BeginMcpLogin {
                    package: settings.package.clone(),
                    server: "input".into(),
                    redirect: "http://127.0.0.1:43191/callback".into(),
                    client_id: Some("different-client".into()),
                });
                assert_eq!(
                    fixture.client.execute(denied).await.unwrap_err().code,
                    ErrorCode::InvalidRequest
                );
                settings.package
            } else {
                package.summary.reference()
            };
            let (_, attempt, url) = begin(&fixture.client, reference).await;
            execute(
                &fixture.client,
                Command::CompleteMcpLogin {
                    attempt: attempt.id,
                    callback: Secret::new(issuer::Server::callback(&url).await),
                },
            )
            .await;
            let State::Connected(connected) = state(&fixture.client, &attempt, |state| {
                matches!(state, State::Connected(_))
            })
            .await
            else {
                unreachable!()
            };
            assert_eq!(
                issuer.registrations.load(Ordering::SeqCst),
                usize::from(!registered)
            );
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
            assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 1);
            assert_eq!(issuer.refreshes.load(Ordering::SeqCst), 0);
            let connected = if registered {
                issuer.replace_secret("replacement-client-private-marker");
                let Output::PluginSettings(saved) = execute(
                    &fixture.client,
                    Command::SavePluginSettings {
                        package: connected,
                        values: BTreeMap::from([("client_id".into(), json!("fixture-client"))]),
                        secrets: BTreeMap::from([(
                            "client_secret".into(),
                            SecretUpdate::Replace(Secret::new(
                                "replacement-client-private-marker".into(),
                            )),
                        )]),
                    },
                )
                .await
                else {
                    panic!("settings expected")
                };
                assert!(saved.ready);
                saved.package
            } else {
                connected
            };
            let process::Fixture {
                directory,
                node,
                controller,
                client,
                session,
                ..
            } = fixture;
            drop(client);
            node.shutdown().await.unwrap();
            let database = rusqlite::Connection::open_with_flags(
                directory.path().join("node/storage/node.sqlite3"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let body: Vec<u8> = database.query_row("SELECT body FROM plugin_settings WHERE plugin='example' ORDER BY revision DESC LIMIT 1", [], |row| row.get(0)).unwrap();
            let stored: Value = serde_json::from_slice(&body).unwrap();
            let credentials: Value = serde_json::from_str(
                stored["authorizations"]["input"]["credentials"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(credentials["client_secret"], "client-private-marker");
            if registered {
                assert_eq!(
                    stored["slots"]["client_secret"],
                    "replacement-client-private-marker"
                );
            }
            drop(database);
            issuer.expire();
            let node = Node::start_empty(directory.path().join("node"))
                .await
                .unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            let Output::McpAuthorization(status) = execute(
                &client,
                Command::ReadMcpAuthorization {
                    package: connected,
                    server: "input".into(),
                },
            )
            .await
            else {
                panic!("authorization expected")
            };
            assert!(status.configured);
            let Output::QueuedTurn(turn) = execute(
                &client,
                Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: "Use the configured MCP tools".into(),
                },
            )
            .await
            else {
                panic!("turn expected")
            };
            let (_, pending) = questions::pending(&client, session.id).await;
            execute(
                &client,
                Command::ResolveQuestion {
                    session: session.id,
                    question: pending.id,
                    response: question::Response::Decline,
                },
            )
            .await;
            let restored = finished(&client, session.id, turn.id).await;
            assert_eq!(restored.runs.last().unwrap().status, Status::Completed);
            assert_eq!(issuer.refreshes.load(Ordering::SeqCst), 1);
            assert_eq!(
                issuer.registrations.load(Ordering::SeqCst),
                usize::from(!registered)
            );
            assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
            let public = serde_json::to_string(&(
                page,
                restored,
                status,
                model.requests.lock().unwrap().clone(),
            ))
            .unwrap();
            for secret in [
                "client-private-marker",
                "replacement-client-private-marker",
                "access-initial",
                "refresh-initial",
                "access-1",
                "refresh-1",
            ] {
                assert!(!public.contains(secret));
            }
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
        }
    }
}
