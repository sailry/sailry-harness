//! Existing credentials and sent captures survive private package binding initialization.
use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retains_legacy_accounts_and_captures() {
    for remote in [false, true] {
        for authentication in [
            Authentication::ChatGpt,
            Authentication::ChatGptOAuth,
            Authentication::ClaudeOAuth,
        ] {
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path == "/responses" || request.path == "/v1/messages" {
                    return Reply::Raw("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 62\r\nConnection: close\r\n\r\n{\"error\":{\"type\":\"invalid_request_error\",\"message\":\"fixture\"}}".into());
                }
                match authentication {
                    Authentication::ChatGpt => standard(request),
                    Authentication::ChatGptOAuth => codex::server::standard(request, 3600),
                    Authentication::ClaudeOAuth => claude::server::standard(request),
                    _ => unreachable!(),
                }
            }).await;
            let fixture = Fixture::empty(remote, &server).await;
            let provider = match authentication {
                Authentication::ChatGpt => {
                    execution::signed_in(&fixture, authentication, ModelApi::Responses).await
                }
                Authentication::ChatGptOAuth => codex::configured(&fixture, None).await,
                Authentication::ClaudeOAuth => claude::connected(&fixture).await,
                _ => unreachable!(),
            };
            let session = execution::session(&fixture, &provider).await;
            let (_, turn) = execution::submit(&fixture.client, &session).await;
            execution::finished(
                &fixture.client,
                &turn,
                sailry_protocol::conversation::Status::Failed,
            )
            .await;
            let capture = fixture
                .client
                .execute(fixture.client.prepare(Command::ReadTurnCapture {
                    session: session.id,
                    turn: turn.id,
                }))
                .await
                .unwrap();
            no_secrets(&capture);
            let credentials = fixture
                .client
                .execute(fixture.client.prepare(Command::ListCredentials))
                .await
                .unwrap();
            let profile = fixture.node.profile().to_owned();
            let reference = provider.credential.as_ref().unwrap();
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
            let (db_handle, db) =
                crate::database_fixture::open(&(profile.join("storage/node.turso")), false).await;
            let body: Vec<u8> = (async {
                let mut statement = db.prepare("SELECT body FROM providers WHERE id=?1").await?;
                let row = statement.query_row([provider.id.to_string()]).await?;
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                mapped
            })
            .await
            .unwrap();
            let mut body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let saved = &mut body["authorizations"][reference.id.to_string()];
            let key = saved["key"].clone();
            assert!(saved.as_object_mut().unwrap().remove("auth").is_some());
            db.execute(
                "UPDATE providers SET body=?2 WHERE id=?1",
                turso::params![provider.id.to_string(), serde_json::to_vec(&body).unwrap()],
            )
            .await
            .unwrap();
            let legacy = serde_json::to_vec(&body).unwrap();
            let frozen: Vec<u8> = (async {
                let mut statement = db
                    .prepare("SELECT provider FROM agent_runs WHERE turn=?1")
                    .await?;
                let row = statement.query_row([turn.id.to_string()]).await?;
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                mapped
            })
            .await
            .unwrap();
            let backups = backups(&profile);
            // Full rehearsal must reject invalid archives before saving or applying metadata.
            let invalid = {
                let transaction = db.unchecked_transaction().await.unwrap();
                let invalid:i64=transaction.prepare("INSERT INTO counters(namespace,value) VALUES('events',1) ON CONFLICT(namespace) DO UPDATE SET value=counters.value+1 WHERE counters.value<9223372036854775807 RETURNING value").await.unwrap().query_row(()).await.unwrap().get(0).unwrap();
                transaction
                    .execute(
                        "INSERT INTO events(cursor,body) VALUES(?1,?2)",
                        turso::params![
                            invalid,
                            br#"{"kind":"invalid_archive","data":null}"#.as_slice()
                        ],
                    )
                    .await
                    .unwrap();
                transaction.commit().await.unwrap();
                invalid
            };
            crate::database_fixture::close(db_handle, db).await;
            let error = Node::start_with_authorization(&profile, &server.endpoint)
                .await
                .err()
                .expect("invalid archive must reject initialization");
            assert!(
                error
                    .to_string()
                    .contains("invalid stored JSON in events.body"),
                "{error}"
            );
            assert_eq!(self::backups(&profile), backups);
            let (db_handle, db) =
                crate::database_fixture::open(&(profile.join("storage/node.turso")), false).await;
            let unchanged: Vec<u8> = (async {
                let mut statement = db.prepare("SELECT body FROM providers WHERE id=?1").await?;
                let row = statement.query_row([provider.id.to_string()]).await?;
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                mapped
            })
            .await
            .unwrap();
            assert_eq!(unchanged, legacy);
            assert_eq!(
                (async {
                    let mut statement = db
                        .prepare("SELECT body FROM events WHERE cursor=?1")
                        .await?;
                    let row = statement.query_row([invalid]).await?;
                    let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<Vec<u8>>(0))(&row);
                    mapped
                })
                .await
                .unwrap(),
                br#"{"kind":"invalid_archive","data":null}"#
            );
            db.execute("DELETE FROM events WHERE cursor=?1", [invalid])
                .await
                .unwrap();
            crate::database_fixture::close(db_handle, db).await;
            let before = server.requests.lock().unwrap().len();
            let node = Node::start_with_authorization(&profile, &server.endpoint)
                .await
                .unwrap();
            let controller = Link::controller(
                fixture.directory.path().join("controller"),
                NetworkScope::default(),
            )
            .await
            .unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            assert_eq!(server.requests.lock().unwrap().len(), before);
            let saved = self::backups(&profile);
            let added = saved.difference(&backups).collect::<Vec<_>>();
            assert_eq!(added.len(), 1);
            let (backup_handle, backup) = crate::database_fixture::open(&(added[0]), true).await;
            assert_eq!(
                (async {
                    let mut statement = backup
                        .prepare("SELECT body FROM providers WHERE id=?1")
                        .await?;
                    let row = statement.query_row([provider.id.to_string()]).await?;
                    let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<Vec<u8>>(0))(&row);
                    mapped
                })
                .await
                .unwrap(),
                legacy
            );
            assert_eq!(
                (async {
                    let mut statement = backup
                        .prepare("SELECT provider FROM agent_runs WHERE turn=?1")
                        .await?;
                    let row = statement.query_row([turn.id.to_string()]).await?;
                    let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<Vec<u8>>(0))(&row);
                    mapped
                })
                .await
                .unwrap(),
                frozen
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    added[0].metadata().unwrap().permissions().mode() & 0o777,
                    0o600
                );
                assert_eq!(
                    added[0]
                        .parent()
                        .unwrap()
                        .metadata()
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o700
                );
            }
            crate::database_fixture::close(backup_handle, backup).await;
            assert_eq!(
                client
                    .execute(client.prepare(Command::ListCredentials))
                    .await
                    .unwrap(),
                credentials
            );
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadTurnCapture {
                        session: session.id,
                        turn: turn.id
                    }))
                    .await
                    .unwrap(),
                capture
            );
            let Output::Providers(providers) = client
                .execute(client.prepare(Command::ListProviders))
                .await
                .unwrap()
            else {
                panic!("providers expected")
            };
            assert_eq!(providers, vec![provider.clone()]);

            let retained: Vec<u8> = {
                let sql = ("SELECT body FROM providers WHERE id=?1").to_owned();
                let parameters = turso::params![provider.id.to_string()];
                node.with_storage(move |db| {
                    Box::pin(async move {
                        let row = db
                            .prepare(sql)
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .query_row(parameters)
                            .await
                            .map_err(crate::database_fixture::fault)?;
                        let value: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                        value.map_err(crate::database_fixture::fault)
                    })
                })
                .await
            }
            .unwrap();
            let retained: serde_json::Value = serde_json::from_slice(&retained).unwrap();
            assert_eq!(
                retained["authorizations"][reference.id.to_string()]["key"],
                key
            );
            assert!(retained["authorizations"][reference.id.to_string()]["auth"].is_object());

            let Output::DiscoveredModels(models) = client
                .execute(client.prepare(Command::DiscoverModels(Box::new(
                    discovery::Source::Saved {
                        provider: provider.id,
                        expected_revision: provider.revision,
                        oauth: None,
                    },
                ))))
                .await
                .unwrap()
            else {
                panic!("models expected")
            };
            assert!(!models.models.is_empty());
            {
                let requests = server.requests.lock().unwrap();
                let reads = &requests[before..];
                assert_eq!(
                    reads.len(),
                    if authentication == Authentication::ClaudeOAuth {
                        2
                    } else {
                        1
                    }
                );
                for (index, request) in reads.iter().enumerate() {
                    assert_eq!(request.method, "GET");
                    assert!(request.body.is_empty());
                    let url = url::Url::parse(&format!("http://fixture{}", request.path)).unwrap();
                    if authentication == Authentication::ClaudeOAuth {
                        assert_eq!(url.path(), "/v1/models");
                        let query = url
                            .query_pairs()
                            .collect::<std::collections::BTreeMap<_, _>>();
                        assert_eq!(query.get("limit").map(|value| value.as_ref()), Some("100"));
                        assert_eq!(
                            query.get("after_id").map(|value| value.as_ref()),
                            if index == 0 { None } else { Some("fixture") }
                        );
                        assert_eq!(query.len(), index + 1);
                    } else {
                        assert_eq!(url.path(), "/models");
                    }
                }
            }
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
            let requests = server.requests.lock().unwrap().len();
            let node = Node::start_with_authorization(&profile, &server.endpoint)
                .await
                .unwrap();
            assert_eq!(self::backups(&profile), saved);
            assert_eq!(server.requests.lock().unwrap().len(), requests);
            let client = Client::new(node.local());
            assert_eq!(
                client
                    .execute(client.prepare(Command::ListCredentials))
                    .await
                    .unwrap(),
                credentials
            );
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadTurnCapture {
                        session: session.id,
                        turn: turn.id,
                    }))
                    .await
                    .unwrap(),
                capture
            );
            node.shutdown().await.unwrap();
        }
    }
}

fn backups(profile: &std::path::Path) -> std::collections::BTreeSet<std::path::PathBuf> {
    match std::fs::read_dir(profile.join("storage/backups")) {
        Ok(entries) => entries
            .map(|entry| entry.unwrap().path().join("node.turso"))
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(error) => panic!("backups cannot be read: {error}"),
    }
}
