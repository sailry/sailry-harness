use super::*;

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
                    // Some refresh responses omit an unchanged refresh token and account claim.
                    let access = format!(
                        "e30.{}.fixture",
                        URL_SAFE_NO_PAD.encode(json!({"exp":future_expiry()}).to_string())
                    );
                    Reply::Json(json!({"access_token":access}))
                } else {
                    let mut value = exchanged(Authentication::ChatGpt, false);
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
        let fixture = Fixture::new(remote, &server).await;
        let provider = signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
        let session = session(&fixture, &provider).await;
        let (_, turn) = submit(&fixture.client, &session).await;
        finished(&fixture.client, &turn, Status::Completed).await;
        assert_eq!(count.load(Ordering::SeqCst), 3);
        assert_eq!(server.requests.lock().unwrap().len(), 7);
        fixture.close().await;
    }
}

async fn restart(fixture: Fixture, server: &Server, remote: bool) -> Fixture {
    let Fixture {
        directory,
        node,
        controller,
        client,
    } = fixture;
    drop(client);
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    let node = Node::start_with_authorization(directory.path().join("node"), &server.endpoint)
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
        for authentication in [Authentication::ChatGpt, Authentication::Copilot] {
            let release = Arc::new(tokio::sync::Notify::new());
            let ready = release.clone();
            let exchanges = Arc::new(AtomicUsize::new(0));
            let count = exchanges.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if exchange(request) {
                    let fresh = count.fetch_add(1, Ordering::SeqCst) > 0;
                    if fresh {
                        Reply::Delayed(ready.clone(), exchanged(authentication, true))
                    } else {
                        Reply::Json(exchanged(authentication, false))
                    }
                } else if request.path == "/responses" {
                    generation(request, authentication, false)
                } else {
                    standard(request)
                }
            })
            .await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = signed_in(&fixture, authentication, ModelApi::Responses).await;
            let first = session(&fixture, &provider).await;
            let second = session(&fixture, &provider).await;
            let (_, stopped) = submit(&fixture.client, &first).await;
            server.wait_requests(4).await;
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
            assert_eq!(server.requests.lock().unwrap().len(), 6);
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
                    if count.fetch_add(1, Ordering::SeqCst) == 0 { return Reply::Json(exchanged(Authentication::ChatGpt, false)); }
                    match mode {
                        "http" => Reply::Raw(format!("HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n{REFRESH}")),
                        "redirect" => Reply::Raw(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {target}/stolen\r\nContent-Length: 0\r\n\r\n")),
                        "account" => {
                            let mut value = exchanged(Authentication::ChatGpt, true);
                            value["access_token"] = json!(format!("e30.{}.fixture", URL_SAFE_NO_PAD.encode(json!({"exp":future_expiry(),"https://api.openai.com/auth":{"chatgpt_account_id":"another-account"}}).to_string())));
                            Reply::Json(value)
                        }
                        "save" => Reply::Json(exchanged(Authentication::ChatGpt, true)),
                        "shutdown" => Reply::Hold,
                        _ => unreachable!(),
                    }
                } else { standard(request) }
            }).await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
            let session = session(&fixture, &provider).await;
            if mode == "save" {
                let db = rusqlite::Connection::open(
                    fixture.directory.path().join("node/storage/node.sqlite3"),
                )
                .unwrap();
                db.execute_batch("CREATE TRIGGER reject_refresh BEFORE UPDATE ON providers WHEN EXISTS(SELECT 1 FROM json_each(NEW.body,'$.authorizations') WHERE json_extract(value,'$.revision')=3) BEGIN SELECT RAISE(ABORT, 'isolated authentication write failure'); END;").unwrap();
            }
            let (_, turn) = submit(&fixture.client, &session).await;
            server.wait_requests(4).await;
            if mode != "shutdown" {
                finished(&fixture.client, &turn, Status::Failed).await;
            }
            let fixture = restart(fixture, &server, remote).await;
            if mode == "shutdown" {
                server.wait_closed(1).await;
            }
            let (_, turn) = submit(&fixture.client, &session).await;
            let page = finished(&fixture.client, &turn, Status::Failed).await;
            let error = page
                .runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .error
                .as_ref()
                .unwrap();
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
                    Reply::Json(exchanged(Authentication::ChatGpt, false))
                } else {
                    Reply::Delayed(ready.clone(), exchanged(Authentication::ChatGpt, true))
                }
            } else {
                standard(request)
            }
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
        let session = session(&fixture, &provider).await;
        let (_, turn) = submit(&fixture.client, &session).await;
        server.wait_requests(4).await;
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
        finished(&fixture.client, &turn, Status::Failed).await;
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
        assert_eq!(server.requests.lock().unwrap().len(), 4);
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
        let fixture = Fixture::new(remote, &server).await;
        let provider = signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
        let session = session(&fixture, &provider).await;
        let db =
            rusqlite::Connection::open(fixture.directory.path().join("node/storage/node.sqlite3"))
                .unwrap();
        db.execute_batch("CREATE TRIGGER reject_admission BEFORE UPDATE ON providers WHEN EXISTS(SELECT 1 FROM json_each(NEW.body,'$.authorizations') WHERE json_extract(value,'$.revision')=2) BEGIN SELECT RAISE(ABORT, 'isolated admission failure'); END;").unwrap();
        let (_, turn) = submit(&fixture.client, &session).await;
        finished(&fixture.client, &turn, Status::Failed).await;
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        db.execute_batch("DROP TRIGGER reject_admission;").unwrap();
        drop(db);
        let (_, turn) = submit(&fixture.client, &session).await;
        finished(&fixture.client, &turn, Status::Completed).await;
        assert_eq!(server.requests.lock().unwrap().len(), 5);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn confines_generation_credentials() {
    for remote in [false, true] {
        for (authentication, api) in [
            (Authentication::ChatGpt, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::ChatCompletions),
        ] {
            for redirect in [false, true] {
                let sink = Server::start_with_request(ModelApi::Anthropic, |_| {
                    panic!("generation redirect must not be followed")
                })
                .await;
                let target = sink.endpoint.clone();
                let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                    if exchange(request) { Reply::Json(exchanged(authentication, true)) }
                    else if matches!(request.path.as_str(), "/responses" | "/chat/completions") {
                        if redirect { Reply::Raw(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {target}/stolen\r\nContent-Length: 0\r\n\r\n")) }
                        else {
                            let body = json!({"error":{"message":ROTATED,"type":"rate_limit_error","code":"rate_limit_exceeded","param":null}});
                            Reply::Raw(format!("HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}"))
                        }
                    } else { standard(request) }
                }).await;
                let fixture = Fixture::new(remote, &server).await;
                let provider = signed_in(&fixture, authentication, api).await;
                let session = session(&fixture, &provider).await;
                let (request, turn) = submit(&fixture.client, &session).await;
                finished(&fixture.client, &turn, Status::Failed).await;
                assert_eq!(
                    fixture.client.execute(request).await.unwrap(),
                    Output::QueuedTurn(turn)
                );
                assert_eq!(
                    server.requests.lock().unwrap().len(),
                    if redirect { 4 } else { 9 },
                    "{authentication:?} {api:?}, remote={remote}, redirect={redirect}",
                );
                assert!(sink.requests.lock().unwrap().is_empty());
                fixture.close().await;
            }
        }
    }
}
