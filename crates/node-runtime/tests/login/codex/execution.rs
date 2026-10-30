use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) async fn captured(fixture: &Fixture, turn: &QueuedTurn) -> Provider {
    let body: Vec<u8> = {
        let sql = ("SELECT provider FROM agent_runs WHERE turn=?1").to_owned();
        let parameters = turso::params![turn.id.to_string()];
        fixture
            .node
            .with_storage(move |db| {
                Box::pin(async move {
                    let row = db
                        .prepare(sql)
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .query_row(parameters)
                        .await
                        .map_err(crate::database_fixture::fault)?;
                    let value: turso::Result<_> = row.get(0);
                    value.map_err(crate::database_fixture::fault)
                })
            })
            .await
    }
    .unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retains_capture_through_refresh() {
    for remote in [false, true] {
        let initial = options("0.161.0", "captured");
        let current = options("0.162.0", "current");
        let expected = initial.clone();
        let latest = current.clone();
        let phase = Arc::new(AtomicUsize::new(0));
        let stage = phase.clone();
        let exchanges = Arc::new(AtomicUsize::new(0));
        let count = exchanges.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            let options = if stage.load(Ordering::SeqCst) == 0 {
                &expected
            } else {
                &latest
            };
            assert_eq!(request.headers["user-agent"], options.user_agent());
            if request.path == "/oauth/token" {
                let index = count.fetch_add(1, Ordering::SeqCst);
                assert!(index < 2, "persisted refresh must not repeat");
                if index == 0 {
                    Reply::Json(server::tokens(request, 30, ACCESS, REFRESH))
                } else {
                    Reply::Json(server::tokens(request, 3600, REFRESHED, ROTATED))
                }
            } else if request.path.starts_with("/models") {
                server::catalog(request, options, ACCESS)
            } else if request.path == "/responses" {
                server::generation(request, options, REFRESHED)
            } else {
                server::standard(request, 30)
            }
        })
        .await;
        let fixture = Fixture::packages(remote, &server, &["files"]).await;
        let mut provider = configured(&fixture, Some(initial.clone())).await;
        provider.models[0].context = 128_000;
        let Output::Provider(provider) = fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let session = super::super::execution::session(&fixture, &provider).await;
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Read the isolated file".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        assert_eq!(captured(&fixture, &turn).await.oauth, Some(initial.clone()));
        let mut changed = provider.clone();
        changed.oauth = Some(current.clone());
        let Output::Provider(changed) = fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider: changed,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let fixture = super::super::execution::renewals::restart(fixture, &server, remote).await;
        assert_eq!(captured(&fixture, &turn).await.oauth, Some(initial));
        assert_eq!(saved(&fixture.client, provider.id).await, changed);
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StartQueuedTurn { turn: turn.id }),
            )
            .await
            .unwrap();
        let queue = fixture
            .client
            .read_conversation(turn.session, None, 10)
            .await
            .unwrap()
            .page
            .queue;
        assert!(queue.paused);
        fixture
            .client
            .execute(fixture.client.prepare(Command::SetQueuePaused {
                session: turn.session,
                expected_revision: queue.revision,
                paused: false,
            }))
            .await
            .unwrap();
        let page =
            super::super::execution::finished(&fixture.client, &turn, Status::Completed).await;
        assert_eq!(answer(&page, turn.id), "Authorized Codex response");
        assert!(
            serde_json::to_string(&page)
                .unwrap()
                .contains("Isolated tool content")
        );
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        phase.store(1, Ordering::SeqCst);
        let fixture = super::super::execution::renewals::restart(fixture, &server, remote).await;

        let stored: Vec<u8> = {
            let sql = ("SELECT body FROM providers WHERE id=?1").to_owned();
            let parameters = turso::params![provider.id.to_string()];
            fixture
                .node
                .with_storage(move |db| {
                    Box::pin(async move {
                        let row = db
                            .prepare(sql)
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .query_row(parameters)
                            .await
                            .map_err(crate::database_fixture::fault)?;
                        let value: turso::Result<_> = row.get(0);
                        value.map_err(crate::database_fixture::fault)
                    })
                })
                .await
        }
        .unwrap();
        let stored: serde_json::Value = serde_json::from_slice(&stored).unwrap();
        let reference = provider.credential.as_ref().unwrap();
        let grant: serde_json::Value = serde_json::from_str(
            stored["authorizations"][reference.id.to_string()]["key"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(grant["refresh"], ROTATED);
        assert_eq!(grant["access"], REFRESHED);

        let (_, next) = super::super::execution::submit(&fixture.client, &session).await;
        let page =
            super::super::execution::finished(&fixture.client, &next, Status::Completed).await;
        assert_eq!(captured(&fixture, &next).await.oauth, Some(current));
        assert_eq!(answer(&page, next.id), "Authorized Codex response");
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request.path == "/responses")
                    .count(),
                4
            );
        }
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn switches_accounts_without_rewriting_captures() {
    for remote in [false, true] {
        for catalog_failure in [false, true] {
            let phase = Arc::new(AtomicUsize::new(0));
            let active = phase.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                let stage = active.load(Ordering::SeqCst);
                let changed = stage != 0;
                if request.path == "/oauth/token" && stage == 1 {
                    return Reply::Hold;
                }
                if request.path == "/oauth/token" && changed {
                    let mut value = server::tokens(request, 3600, "account-b-access", "account-b-refresh");
                    value["id_token"] = json!(super::super::identity::signed(&json!({
                        "sub":"account-b-subject","name":"Account B","email":"b@example.test",
                        "chatgpt_account_id":"account-b-workspace"
                    })));
                    return Reply::Json(value);
                }
                if request.path.starts_with("/models") && changed {
                    assert_eq!(request.headers["authorization"], "Bearer account-b-access");
                    assert_eq!(request.headers["chatgpt-account-id"], "account-b-workspace");
                    if catalog_failure {
                        return Reply::Raw("HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
                    }
                    return Reply::Json(json!({"models":[{"slug":"fixture","display_name":"Fixture","visibility":"list"},{"slug":"b-only","display_name":"Account B model","visibility":"list"}]}));
                }
                if request.path == "/responses" {
                    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
                    let from_b = request.headers["authorization"] == "Bearer account-b-access";
                    assert_eq!(request.headers["chatgpt-account-id"], if from_b {"account-b-workspace"} else {ACCOUNT});
                    if !from_b { assert_eq!(request.headers["authorization"], format!("Bearer {ACCESS}")); }
                    let completed = json!({"type":"response.completed","sequence_number":0,"response":{"id":"fixture-response","object":"response","created_at":1,"status":"completed","model":body["model"],"output":[{"type":"message","id":"fixture-message","role":"assistant","status":"completed","content":[{"type":"output_text","text":if from_b {"Account B response"} else {"Account A response"},"annotations":[]}]}]}});
                    return Reply::Raw(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {completed}\n\n"));
                }
                server::standard(request, 3600)
            }).await;
            let fixture = Fixture::empty(remote, &server).await;
            let mut provider = configured(&fixture, None).await;
            provider.models[0].context = 8192;
            let mut extra = provider.models[0].clone();
            extra.id = "unknown".into();
            provider.models.push(extra);
            provider.default_model = "unknown".into();
            let Output::Provider(provider) = fixture
                .client
                .execute(fixture.client.prepare(Command::PutProvider {
                    expected_revision: provider.revision,
                    provider,
                }))
                .await
                .unwrap()
            else {
                panic!("provider expected")
            };
            let mut selected = provider.clone();
            selected.models.swap(0, 1);
            let session = super::super::execution::session(&fixture, &selected).await;
            let Output::QueuedTurn(queued) = fixture
                .client
                .execute(fixture.client.prepare(Command::QueueTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: "Account A queued input".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("queued turn expected")
            };
            let frozen = captured(&fixture, &queued).await;
            let Output::Credentials(original) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListCredentials))
                .await
                .unwrap()
            else {
                panic!("credentials expected")
            };
            phase.store(1, Ordering::SeqCst);
            let (_, cancelled) = start(&fixture.client, &provider).await;
            let mut pending = fixture.client.subscribe_login(cancelled.id).await.unwrap();
            fixture
                .client
                .execute(fixture.client.prepare(Command::CancelProviderLogin {
                    attempt: cancelled.id,
                }))
                .await
                .unwrap();
            assert_eq!(terminal(&mut pending).await.state, login::State::Cancelled);
            assert_eq!(saved(&fixture.client, provider.id).await, provider);
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ListCredentials))
                    .await
                    .unwrap(),
                Output::Credentials(original.clone())
            );
            phase.store(2, Ordering::SeqCst);
            let (_, attempt) = begin(&fixture.client, &provider).await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            let update = terminal(&mut stream).await;
            assert_eq!(update.state, login::State::Connected);
            assert_eq!(update.model_error.is_some(), catalog_failure);
            let replaced = saved(&fixture.client, provider.id).await;
            assert_eq!(replaced.id, provider.id);
            assert_eq!(replaced.name, provider.name);
            assert_eq!(replaced.oauth, provider.oauth);
            assert_ne!(replaced.credential, provider.credential);
            if catalog_failure {
                assert!(replaced.models.is_empty());
                assert!(replaced.default_model.is_empty());
            } else {
                assert_eq!(replaced.models.len(), 2);
                assert_eq!(replaced.models[0].id, "fixture");
                assert_eq!(replaced.models[0].context, 8192);
                assert_eq!(replaced.models[1].id, "b-only");
                assert_eq!(replaced.default_model, "fixture");
            }
            let Output::Snapshot(snapshot) = fixture
                .client
                .execute(fixture.client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            let current = snapshot
                .sessions
                .into_iter()
                .find(|value| value.id == session.id)
                .unwrap();
            assert_eq!(current.revision, session.revision + 1);
            assert_eq!(current.config.credential, replaced.credential);
            assert_eq!(
                current.config.model,
                if catalog_failure {
                    "unknown"
                } else {
                    "fixture"
                }
            );
            assert_eq!(captured(&fixture, &queued).await, frozen);
            let Output::QueuedTurn(retained) = fixture
                .client
                .execute(fixture.client.prepare(Command::ReadTurnCapture {
                    session: session.id,
                    turn: queued.id,
                }))
                .await
                .unwrap()
            else {
                panic!("capture expected")
            };
            assert_eq!(retained, queued);
            let Output::Credentials(credentials) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListCredentials))
                .await
                .unwrap()
            else {
                panic!("credentials expected")
            };
            assert_eq!(credentials.len(), 2);
            assert!(credentials.contains(&original[0]));
            assert_eq!(
                credentials
                    .iter()
                    .find(|value| Some(value.id)
                        == replaced.credential.as_ref().map(|value| value.id))
                    .unwrap()
                    .account_label
                    .as_deref(),
                Some("b@example.test")
            );
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::SubmitTurn {
                        session: session.id,
                        expected_revision: session.revision,
                        message: "Stale configuration".into()
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::RevisionConflict
            );
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::StartQueuedTurn { turn: queued.id }),
                )
                .await
                .unwrap();
            let queue = fixture
                .client
                .read_conversation(session.id, None, 10)
                .await
                .unwrap()
                .page
                .queue;
            if queue.paused {
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::SetQueuePaused {
                        session: session.id,
                        expected_revision: queue.revision,
                        paused: false,
                    }))
                    .await
                    .unwrap();
            }
            let completed =
                super::super::execution::finished(&fixture.client, &queued, Status::Completed)
                    .await;
            assert_eq!(answer(&completed, queued.id), "Account A response");
            let (_, next) = super::super::execution::submit(&fixture.client, &current).await;
            let page =
                super::super::execution::finished(&fixture.client, &next, Status::Completed).await;
            assert_eq!(answer(&page, next.id), "Account B response");
            assert_eq!(answer(&page, queued.id), "Account A response");
            assert_eq!(
                captured(&fixture, &next).await.credential,
                replaced.credential
            );
            assert_eq!(captured(&fixture, &queued).await, frozen);
            let fixture =
                super::super::execution::renewals::restart(fixture, &server, remote).await;
            assert_eq!(saved(&fixture.client, provider.id).await, replaced);
            assert_eq!(captured(&fixture, &queued).await, frozen);
            assert_eq!(
                server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|value| value.path == "/responses")
                    .count(),
                2
            );
            fixture.close().await;
        }
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preserves_unsettled_authorizations_on_relogin() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, |request| {
            server::standard(request, 3600)
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = configured(&fixture, None).await;
        let session = super::super::execution::session(&fixture, &provider).await;
        let Output::QueuedTurn(queued) = fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Retain uncertain authorization".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        let reference = provider.credential.as_ref().unwrap();

        async fn read(node: &Node, id: ProviderId) -> serde_json::Value {
            let body: Vec<u8> = node
                .with_storage(move |db| {
                    Box::pin(async move {
                        db.prepare("SELECT body FROM providers WHERE id=?1")
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .query_row([id.to_string()])
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .get(0)
                            .map_err(crate::database_fixture::fault)
                    })
                })
                .await
                .unwrap();
            serde_json::from_slice(&body).unwrap()
        }
        let mut stored = read(&fixture.node, provider.id).await;
        let authorization = &mut stored["authorizations"][reference.id.to_string()];
        let mut grant: serde_json::Value =
            serde_json::from_str(authorization["key"].as_str().unwrap()).unwrap();
        grant["renewal_pending"] = json!(true);
        authorization["key"] = json!(grant.to_string());
        let original = authorization.clone();
        {
            let sql = ("UPDATE providers SET body=?1 WHERE id=?2").to_owned();
            let parameters = turso::params![
                serde_json::to_vec(&stored).unwrap(),
                provider.id.to_string()
            ];
            fixture
                .node
                .with_storage(move |db| {
                    Box::pin(async move {
                        db.execute(sql, parameters)
                            .await
                            .map_err(crate::database_fixture::fault)
                    })
                })
                .await
        }
        .unwrap();

        let fixture = super::super::execution::renewals::restart(fixture, &server, remote).await;
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        let update = terminal(&mut stream).await;
        assert_eq!(update.state, login::State::Connected);
        assert_eq!(update.model_error, None);
        let current = saved(&fixture.client, provider.id).await;
        assert_ne!(current.credential, provider.credential);

        let stored = read(&fixture.node, provider.id).await;
        assert_eq!(stored["authorizations"][reference.id.to_string()], original);
        let current_grant: serde_json::Value = serde_json::from_str(
            stored["authorizations"][current.credential.as_ref().unwrap().id.to_string()]["key"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(current_grant["renewal_pending"], false);
        assert_eq!(
            captured(&fixture, &queued).await.credential,
            provider.credential
        );
        let Output::QueuedTurn(retained) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadTurnCapture {
                session: session.id,
                turn: queued.id,
            }))
            .await
            .unwrap()
        else {
            panic!("capture expected")
        };
        assert_eq!(retained, queued);

        fixture.close().await;
    }
}
