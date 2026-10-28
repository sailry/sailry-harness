use super::*;
use sailry_client::{Apply, Projection};

pub(super) async fn changed(
    subscription: &mut Box<dyn Subscription>,
    projection: &mut Projection,
    reference: &CredentialRef,
    revision: u64,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let update = subscription.next().await.unwrap();
            no_secrets(&update);
            let covered = matches!(&update, Update::Event(envelope)
                if envelope.cursor <= projection.snapshot().unwrap().cursor);
            let observed = matches!(&update, Update::Event(envelope)
                if matches!(&envelope.event, Event::CredentialChanged {credential, revision: current}
                    if credential == reference && *current == revision));
            assert_eq!(projection.apply(0, update).unwrap(), if covered {Apply::Ignored} else {Apply::Applied});
            if observed && !covered {return;}
        }
    }).await.expect("credential event deadline");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refreshes_expired_access_after_restart() {
    for remote in [false, true] {
        let exchanges = Arc::new(AtomicUsize::new(0));
        let count = exchanges.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if exchange(request) {
                count.fetch_add(1, Ordering::SeqCst);
                Reply::Json(exchanged(request, Authentication::ChatGpt, true))
            } else if request.path == "/responses" {
                generation(request, Authentication::ChatGpt, false)
            } else {
                standard(request)
            }
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
        let reference = provider.credential.as_ref().unwrap();

        let mut body: serde_json::Value = serde_json::from_str(
            &{
                let sql = ("SELECT CAST(body AS TEXT) FROM providers WHERE id=?1").to_owned();
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
                            let value: turso::Result<_> = row.get::<String>(0);
                            value.map_err(crate::database_fixture::fault)
                        })
                    })
                    .await
            }
            .unwrap(),
        )
        .unwrap();
        let saved = &mut body["authorizations"][reference.id.to_string()];
        assert!(saved["expires_at_ms"].is_null());
        let mut grant: serde_json::Value =
            serde_json::from_str(saved["key"].as_str().unwrap()).unwrap();
        grant["expires_at_ms"] = json!((future_expiry() - 3601) * 1000);
        saved["key"] = json!(grant.to_string());
        {
            let sql = ("UPDATE providers SET body=?1 WHERE id=?2").to_owned();
            let parameters =
                turso::params![serde_json::to_vec(&body).unwrap(), provider.id.to_string()];
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

        let fixture = restart(fixture, &server, remote).await;
        let mut subscription = fixture.client.subscribe().await.unwrap();
        let mut projection = Projection::new(fixture.client.target(), 0);
        projection
            .apply(0, subscription.next().await.unwrap())
            .unwrap();
        assert!(projection.snapshot().unwrap().provider_quotas.is_empty());
        let reference = provider.credential.clone().unwrap();
        let Output::Credentials(credentials) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListCredentials))
            .await
            .unwrap()
        else {
            panic!("credentials expected")
        };
        let revision = credentials
            .iter()
            .find(|value| value.id == reference.id)
            .unwrap()
            .revision;
        discover(&fixture, &provider).await.unwrap();
        changed(&mut subscription, &mut projection, &reference, revision + 1).await;
        changed(&mut subscription, &mut projection, &reference, revision + 2).await;
        assert_eq!(
            projection.snapshot().unwrap().providers,
            vec![provider.clone()]
        );
        assert!(projection.snapshot().unwrap().provider_quotas.is_empty());
        drop(subscription);
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        assert_eq!(server.requests.lock().unwrap().len(), 6);
        let fixture = restart(fixture, &server, remote).await;
        discover(&fixture, &provider).await.unwrap();
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        assert_eq!(server.requests.lock().unwrap().len(), 7);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retains_rotated_grants() {
    for remote in [false, true] {
        let count = Arc::new(AtomicUsize::new(0));
        let exchanges = count.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if exchange(request) {
                let index = exchanges.fetch_add(1, Ordering::SeqCst);
                if index == 2 {
                    assert!(request.body.contains(ROTATED));
                    assert!(!request.body.contains(REFRESH));
                    // Refresh rotates both tokens while identity and scopes may be omitted.
                    let mut value = exchanged(request, Authentication::ChatGpt, true);
                    value.as_object_mut().unwrap().remove("scope");
                    Reply::Json(value)
                } else {
                    let mut value = exchanged(request, Authentication::ChatGpt, false);
                    if index == 1 {
                        value["refresh_token"] = json!(ROTATED);
                    }
                    Reply::Json(value)
                }
            } else if request.path == "/responses" {
                generation(request, Authentication::ChatGpt, true)
            } else {
                standard(request)
            }
        })
        .await;
        let fixture = Fixture::packages(remote, &server, &["files"]).await;
        let provider = signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
        discover(&fixture, &provider).await.unwrap();
        discover(&fixture, &provider).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 3);
        assert_eq!(server.requests.lock().unwrap().len(), 8);
        fixture.close().await;
    }
}

pub(in super::super) async fn restart(fixture: Fixture, server: &Server, remote: bool) -> Fixture {
    let Fixture {
        directory,
        node,
        controller,
        client,
    } = fixture;
    drop(client);
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    let node =
        Node::start_empty_with_authorization(directory.path().join("node"), &server.endpoint)
            .await
            .unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let client = Arc::new(Client::new(if remote {
        controller.handle().remote(node.link().address())
    } else {
        node.local()
    }));
    Fixture {
        directory,
        node,
        controller,
        client,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refresh_survives_turn_cancellation() {
    for remote in [false, true] {
        for authentication in [Authentication::Copilot] {
            let release = Arc::new(tokio::sync::Notify::new());
            let ready = release.clone();
            let exchanges = Arc::new(AtomicUsize::new(0));
            let count = exchanges.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if exchange(request) {
                    let fresh = count.fetch_add(1, Ordering::SeqCst) > 0;
                    if fresh {
                        Reply::Delayed(ready.clone(), exchanged(request, authentication, true))
                    } else {
                        Reply::Json(exchanged(request, authentication, false))
                    }
                } else if request.path == "/responses" {
                    generation(request, authentication, false)
                } else {
                    standard(request)
                }
            })
            .await;
            let fixture = Fixture::empty(remote, &server).await;
            let provider = signed_in(&fixture, authentication, ModelApi::Responses).await;
            let first = session(&fixture, &provider).await;
            let second = session(&fixture, &provider).await;
            let (_, stopped) = submit(&fixture.client, &first).await;
            server.wait_requests(5).await;
            let (_, waiting) = submit(&fixture.client, &second).await;
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::StopTurn { turn: stopped.id }),
                )
                .await
                .unwrap();
            finished(&fixture.client, &stopped, Status::Cancelled).await;
            assert_eq!(exchanges.load(Ordering::SeqCst), 2);
            release.notify_one();
            finished(&fixture.client, &waiting, Status::Completed).await;
            let fixture = restart(fixture, &server, remote).await;
            let (_, turn) = submit(&fixture.client, &first).await;
            finished(&fixture.client, &turn, Status::Completed).await;
            assert_eq!(exchanges.load(Ordering::SeqCst), 2);
            assert_eq!(server.requests.lock().unwrap().len(), 7);
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preserves_uncertain_rotation() {
    for remote in [false, true] {
        for mode in ["http", "redirect", "account", "save", "shutdown"] {
            let exchanges = Arc::new(AtomicUsize::new(0));
            let count = exchanges.clone();
            let sink = Server::start_with_request(ModelApi::Anthropic, |_| {
                panic!("authorization redirect must not be followed")
            })
            .await;
            let target = sink.endpoint.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if exchange(request) {
                    if count.fetch_add(1, Ordering::SeqCst) == 0 { return Reply::Json(exchanged(request, Authentication::ChatGpt, false)); }
                    match mode {
                        "http" => Reply::Raw(format!("HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n{REFRESH}")),
                        "redirect" => Reply::Raw(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {target}/stolen\r\nContent-Length: 0\r\n\r\n")),
                        "account" => {
                            let mut value = exchanged(request, Authentication::ChatGpt, true);
                            value["id_token"] = json!(identity::signed(&json!({"iss":identity::issuer(request),
                                "aud":identity::CLIENT,"sub":"another-subject","iat":future_expiry()-3600,"exp":future_expiry()})));
                            Reply::Json(value)
                        }
                        "save" => Reply::Json(exchanged(request, Authentication::ChatGpt, true)),
                        "shutdown" => Reply::Hold,
                        _ => unreachable!(),
                    }
                } else { standard(request) }
            }).await;
            let fixture = Fixture::empty(remote, &server).await;
            let provider = signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
            if mode == "save" {
                { let sql = ("CREATE TRIGGER reject_refresh BEFORE UPDATE ON providers WHEN EXISTS(SELECT 1 FROM json_each(NEW.body,'$.authorizations') WHERE json_extract(value,'$.revision')=3) BEGIN SELECT RAISE(ABORT, 'isolated authentication write failure'); END;").to_owned();
fixture.node.with_storage(move |db| Box::pin(async move { db.execute_batch(sql).await.map_err(crate::database_fixture::fault) })).await }.unwrap();
            }
            let pending = discovering(&fixture, &provider);
            server.wait_requests(5).await;
            let mut pending = Some(pending);
            if mode != "shutdown" {
                assert!(pending.take().unwrap().await.unwrap().is_err());
            }
            if mode == "save" {
                {
                    let sql = ("DROP TRIGGER reject_refresh;").to_owned();
                    fixture
                        .node
                        .with_storage(move |db| {
                            Box::pin(async move {
                                db.execute_batch(sql)
                                    .await
                                    .map_err(crate::database_fixture::fault)
                            })
                        })
                        .await
                }
                .unwrap();
            }
            let fixture = restart(fixture, &server, remote).await;
            if mode == "shutdown" {
                server.wait_closed(1).await;
                let _ = pending.take().unwrap().await;
            }
            let error = discover(&fixture, &provider).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::OutcomeUnknown, "{mode}: {error:?}");
            assert_eq!(exchanges.load(Ordering::SeqCst), 2);
            assert!(sink.requests.lock().unwrap().is_empty());
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preserves_revocation_during_refresh() {
    for remote in [false, true] {
        let release = Arc::new(tokio::sync::Notify::new());
        let ready = release.clone();
        let count = AtomicUsize::new(0);
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if exchange(request) {
                if count.fetch_add(1, Ordering::SeqCst) == 0 {
                    Reply::Json(exchanged(request, Authentication::ChatGpt, false))
                } else {
                    Reply::Delayed(
                        ready.clone(),
                        exchanged(request, Authentication::ChatGpt, true),
                    )
                }
            } else {
                standard(request)
            }
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
        let pending = discovering(&fixture, &provider);
        server.wait_requests(5).await;
        let reference = provider.credential.unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::RevokeCredential {
                id: reference.id,
                expected_revision: 2,
            }))
            .await
            .unwrap();
        release.notify_one();
        assert!(pending.await.unwrap().is_err());
        let Output::Credentials(credentials) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListCredentials))
            .await
            .unwrap()
        else {
            panic!("credentials expected")
        };
        assert!(credentials[0].revoked);
        assert_eq!(credentials[0].revision, 3);
        assert_eq!(server.requests.lock().unwrap().len(), 5);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admits_before_rotation() {
    for remote in [false, true] {
        let count = AtomicUsize::new(0);
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if exchange(request) {
                Reply::Json(exchanged(
                    request,
                    Authentication::ChatGpt,
                    count.fetch_add(1, Ordering::SeqCst) > 0,
                ))
            } else if request.path == "/responses" {
                generation(request, Authentication::ChatGpt, false)
            } else {
                standard(request)
            }
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;

        { let sql = ("CREATE TRIGGER reject_admission BEFORE UPDATE ON providers WHEN EXISTS(SELECT 1 FROM json_each(NEW.body,'$.authorizations') WHERE json_extract(value,'$.revision')=2) BEGIN SELECT RAISE(ABORT, 'isolated admission failure'); END;").to_owned();
fixture.node.with_storage(move |db| Box::pin(async move { db.execute_batch(sql).await.map_err(crate::database_fixture::fault) })).await }.unwrap();
        assert!(discover(&fixture, &provider).await.is_err());
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        {
            let sql = ("DROP TRIGGER reject_admission;").to_owned();
            fixture
                .node
                .with_storage(move |db| {
                    Box::pin(async move {
                        db.execute_batch(sql)
                            .await
                            .map_err(crate::database_fixture::fault)
                    })
                })
                .await
        }
        .unwrap();

        discover(&fixture, &provider).await.unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), 6);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn confines_generation_credentials() {
    let selected = Arc::new(std::sync::Mutex::new((Authentication::ChatGpt, false)));
    let current = selected.clone();
    let sink = Server::start_with_request(ModelApi::Anthropic, |_| {
        panic!("generation redirect must not be followed")
    })
    .await;
    let target = sink.endpoint.clone();
    let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
        let (authentication, redirect) = *current.lock().unwrap();
        if exchange(request) { Reply::Json(exchanged(request, authentication, true)) }
        else if matches!(request.path.as_str(), "/responses" | "/chat/completions") {
            if redirect { Reply::Raw(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {target}/stolen\r\nContent-Length: 0\r\n\r\n")) }
            else {
                let echoed = if authentication == Authentication::ChatGpt {
                    ROTATED.to_owned()
                } else {
                    request.headers.get("authorization").unwrap().clone()
                };
                let body = json!({"error":{"message":echoed,"type":"rate_limit_error","code":"rate_limit_exceeded","param":null}});
                Reply::Raw(format!("HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nRetry-After: 0\r\nConnection: close\r\n\r\n{body}"))
            }
        } else { standard(request) }
    }).await;
    let mut fixture = Fixture::empty(false, &server).await;
    let mut credentials = std::collections::BTreeSet::new();
    for (authentication, api) in [
        (Authentication::Copilot, ModelApi::Responses),
        (Authentication::Copilot, ModelApi::ChatCompletions),
    ] {
        *selected.lock().unwrap() = (authentication, false);
        fixture.client = Arc::new(Client::new(fixture.node.local()));
        let before = server.requests.lock().unwrap().len();
        let provider = signed_in(&fixture, authentication, api).await;
        let authorization_requests = server.requests.lock().unwrap().len() - before;
        assert_eq!(authorization_requests, 4);
        assert!(credentials.insert(provider.credential.as_ref().unwrap().id));
        for remote in [false, true] {
            fixture.client = Arc::new(Client::new(if remote {
                fixture
                    .controller
                    .handle()
                    .remote(fixture.node.link().address())
            } else {
                fixture.node.local()
            }));
            for redirect in [false, true] {
                *selected.lock().unwrap() = (authentication, redirect);
                let before = server.requests.lock().unwrap().len();
                let session = session(&fixture, &provider).await;
                let (request, turn) = submit(&fixture.client, &session).await;
                let page = finished_within(
                    &fixture.client,
                    &turn,
                    Status::Failed,
                    crate::retry_fixture::budget(),
                )
                .await;
                no_secrets(&page);
                let public = serde_json::to_string(&page).unwrap();
                for secret in [ROTATED, "renewed-access"] {
                    assert!(!public.contains(secret));
                }
                if !redirect && api == ModelApi::Responses {
                    let error = page.runs.last().unwrap().error.as_ref().unwrap();
                    assert_eq!(error.message, "Rate limit exceeded: [redacted]");
                }
                assert!(page.entries.iter().flat_map(|entry| &entry.parts).all(
                    |part| !matches!(part, Part::Resource(value) if value["type"] == "model_retry")
                ));
                assert_eq!(
                    fixture.client.execute(request).await.unwrap(),
                    Output::QueuedTurn(turn)
                );
                assert_eq!(
                    authorization_requests + server.requests.lock().unwrap().len() - before,
                    4 + 1 + goose_provider_types::retry::DEFAULT_MAX_RETRIES,
                    "{authentication:?} {api:?}, remote={remote}, redirect={redirect}",
                );
                assert!(sink.requests.lock().unwrap().is_empty());
            }
        }
    }
    fixture.close().await;
}

fn discovering(
    fixture: &Fixture,
    provider: &Provider,
) -> tokio::task::JoinHandle<Result<Output, Fault>> {
    let client = fixture.client.clone();
    let request = client.prepare(Command::DiscoverModels(Box::new(
        discovery::Source::Saved {
            provider: provider.id,
            expected_revision: provider.revision,
            oauth: None,
        },
    )));
    tokio::spawn(async move { client.execute(request).await })
}
