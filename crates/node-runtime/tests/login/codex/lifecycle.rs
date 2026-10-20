use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connects_with_isolated_credentials() {
    for remote in [false, true] {
        for outcome in ["connected", "cancelled", "denied", "expired"] {
            let release = Arc::new(tokio::sync::Notify::new());
            let gate = release.clone();
            let polls = AtomicUsize::new(0);
            let drift = Arc::new(AtomicUsize::new(0));
            let identity = drift.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                assert!(
                    !request.path.starts_with("/.well-known/"),
                    "Codex authorization does not request OIDC discovery"
                );
                if request.path == "/api/accounts/deviceauth/usercode" && outcome == "denied" {
                    return Reply::Raw("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
                }
                if request.path == "/api/accounts/deviceauth/token" {
                    let Reply::Json(value) = server::standard(request, 3600) else {
                        panic!("device exchange expected")
                    };
                    return match outcome {
                        "connected" if polls.fetch_add(1, Ordering::SeqCst) == 0 => Reply::Raw("HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()),
                        "connected" => Reply::Delayed(gate.clone(), value),
                        "cancelled" => Reply::Hold,
                        "expired" => Reply::Raw("HTTP/1.1 410 Gone\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()),
                        _ => unreachable!(),
                    };
                }
                if request.path == "/oauth/token" && identity.load(Ordering::SeqCst) > 0 {
                    let index = identity.load(Ordering::SeqCst);
                    let mut value = server::tokens(request, 3600, ACCESS, REFRESH);
                    value["id_token"] = json!(super::super::identity::signed(&json!({
                        "iss":super::super::identity::issuer(request),"aud":CLIENT,
                        "sub":if index == 1 { "changed-codex-subject" } else { "isolated-codex-subject" },
                        "email":"codex@example.com","iat":future_expiry()-3600,"exp":future_expiry(),
                        "https://api.openai.com/auth":{"chatgpt_account_id":if index == 2 { "changed-codex-workspace" } else { ACCOUNT }}
                    })));
                    return Reply::Json(value);
                }
                if request.path.starts_with("/models") && identity.load(Ordering::SeqCst) == 2 {
                    assert_eq!(request.headers["chatgpt-account-id"], "changed-codex-workspace");
                    assert_eq!(request.headers["authorization"], format!("Bearer {ACCESS}"));
                    return Reply::Json(server::models());
                }
                server::standard(request, 3600)
            }).await;
            let fixture = Fixture::empty(remote, &server).await;
            let provider = provider(&fixture.client, Authentication::ChatGptOAuth).await;
            let (request, attempt) = start(&fixture.client, &provider).await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            if matches!(outcome, "connected" | "cancelled") {
                loop {
                    let update = next(&mut stream).await;
                    no_secrets(&update);
                    match update.state {
                        login::State::Starting => {}
                        login::State::Pending {
                            verification_url,
                            user_code,
                            ..
                        } => {
                            assert_eq!(verification_url, "https://auth.openai.com/codex/device");
                            assert_eq!(user_code, USER_CODE);
                            break;
                        }
                        state => panic!("device prompt expected, got {state:?}"),
                    }
                }
                if outcome == "connected" {
                    release.notify_one();
                } else {
                    server.wait_requests(2).await;
                    let cancel = fixture.client.prepare(Command::CancelProviderLogin {
                        attempt: attempt.id,
                    });
                    let result = fixture.client.execute(cancel.clone()).await.unwrap();
                    assert_eq!(fixture.client.execute(cancel).await.unwrap(), result);
                }
            }
            let update = terminal(&mut stream).await;
            no_secrets(&update);
            drop(stream);
            if outcome == "connected" {
                assert_eq!(update.state, login::State::Connected);
                assert_eq!(update.model_error, None);
                let connected = saved(&fixture.client, provider.id).await;
                assert_eq!(connected.authentication, Authentication::ChatGptOAuth);
                assert_eq!(connected.endpoint, "https://chatgpt.com/backend-api/codex");
                assert_eq!(
                    connected
                        .models
                        .iter()
                        .map(|model| model.id.as_str())
                        .collect::<Vec<_>>(),
                    ["fixture", "unknown"]
                );
                let reference = connected.credential.clone().unwrap();
                let Output::Credentials(credentials) = fixture
                    .client
                    .execute(fixture.client.prepare(Command::ListCredentials))
                    .await
                    .unwrap()
                else {
                    panic!("credentials expected")
                };
                no_secrets(&credentials);
                assert_eq!(credentials.len(), 1);
                assert_eq!(credentials[0].authentication, Authentication::ChatGptOAuth);
                assert_eq!(
                    credentials[0].account_label.as_deref(),
                    Some("codex@example.com")
                );
                assert_eq!(
                    fixture
                        .client
                        .execute(fixture.client.prepare(Command::ReadProviderKey {
                            provider: connected.id,
                            expected_revision: connected.revision,
                        }))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::PermissionDenied
                );
                assert_eq!(
                    fixture
                        .node
                        .resolve_credential(reference.clone(), connected.id)
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::PermissionDenied
                );
                assert_eq!(
                    fixture
                        .client
                        .execute(fixture.client.prepare(Command::DiscoverModels(Box::new(
                            discovery::Source::Draft(discovery::Draft {
                                preset: None,
                                provider: connected.id,
                                api: ModelApi::Responses,
                                endpoint: server.endpoint.clone(),
                                credential: Some(reference),
                                secret: None,
                            })
                        ))))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::PermissionDenied
                );
                let Output::DiscoveredModels(discovered) = fixture
                    .client
                    .execute(fixture.client.prepare(Command::DiscoverModels(Box::new(
                        discovery::Source::Saved {
                            provider: connected.id,
                            expected_revision: connected.revision,
                            oauth: None,
                        },
                    ))))
                    .await
                    .unwrap()
                else {
                    panic!("models expected")
                };
                assert_eq!(discovered.models.len(), 2);
                let before = server.requests.lock().unwrap().len();
                assert_eq!(
                    fixture.client.execute(request.clone()).await.unwrap(),
                    Output::ProviderLogin(attempt.clone())
                );
                assert_eq!(server.requests.lock().unwrap().len(), before);
                let fixture =
                    super::super::execution::renewals::restart(fixture, &server, remote).await;
                assert_eq!(saved(&fixture.client, provider.id).await, connected);
                assert_eq!(
                    fixture.client.execute(request).await.unwrap(),
                    Output::ProviderLogin(attempt)
                );
                assert_eq!(server.requests.lock().unwrap().len(), before);
                let mut current = connected.clone();
                for index in [1, 2] {
                    drift.store(index, Ordering::SeqCst);
                    release.notify_one();
                    let (_, reconnect) = start(&fixture.client, &current).await;
                    let mut stream = fixture.client.subscribe_login(reconnect.id).await.unwrap();
                    let update = terminal(&mut stream).await;
                    assert_eq!(update.state, login::State::Connected);
                    assert_eq!(update.model_error, None);
                    let replaced = saved(&fixture.client, provider.id).await;
                    assert_eq!(replaced.id, current.id);
                    assert_eq!(replaced.revision, current.revision + 1);
                    assert_ne!(replaced.credential, current.credential);
                    let Output::Credentials(retained) = fixture
                        .client
                        .execute(fixture.client.prepare(Command::ListCredentials))
                        .await
                        .unwrap()
                    else {
                        panic!("credentials expected")
                    };
                    assert_eq!(retained.len(), index + 1);
                    assert!(retained.contains(&credentials[0]));
                    assert!(retained.iter().all(|credential| !credential.revoked));
                    current = replaced;
                }
                no_secrets(
                    &fixture
                        .client
                        .execute(fixture.client.prepare(Command::Snapshot))
                        .await
                        .unwrap(),
                );
                fixture.close().await;
            } else {
                match outcome {
                    "cancelled" => {
                        assert_eq!(update.state, login::State::Cancelled);
                        server.wait_closed(1).await;
                    }
                    "denied" => {
                        let login::State::Failed(error) = update.state else {
                            panic!("denial expected")
                        };
                        assert_eq!(error.code, ErrorCode::PermissionDenied);
                    }
                    "expired" => {
                        let login::State::Failed(error) = update.state else {
                            panic!("expiry expected")
                        };
                        assert_eq!(error.code, ErrorCode::Expired);
                    }
                    _ => unreachable!(),
                }
                assert_eq!(
                    fixture
                        .client
                        .execute(fixture.client.prepare(Command::ListCredentials))
                        .await
                        .unwrap(),
                    Output::Credentials(vec![])
                );
                assert_eq!(saved(&fixture.client, provider.id).await, provider);
                assert!(
                    !server
                        .requests
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|request| request.path == "/oauth/token"
                            || request.path.starts_with("/models"))
                );
                fixture.close().await;
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preserves_pending_controls() {
    for remote in [false, true] {
        for outcome in ["connected", "cancelled", "expired", "denied"] {
            let polls = Arc::new(AtomicUsize::new(0));
            let count = polls.clone();
            let server = Server::start_with_request(ModelApi::Anthropic,move |request| {
                if request.path == "/api/accounts/deviceauth/usercode" {
                    let Reply::Json(mut value) = server::standard(request,3600) else {panic!("device prompt expected")};
                    let code = value.as_object_mut().unwrap().remove("user_code").unwrap();
                    value["usercode"] = code;
                    if outcome == "expired" {value["expires_in"] = json!(3);}
                    return Reply::Json(value);
                }
                if request.path == "/api/accounts/deviceauth/token" {
                    let index = count.fetch_add(1,Ordering::SeqCst);
                    let error = if outcome == "denied" {Some((400,json!({"error":{"code":"access_denied"}})))}
                        else if outcome == "expired" || index == 1 {Some((429,json!({"error":{"code":"slow_down"}})))}
                        else if index == 0 {Some((400,json!({"error":"deviceauth_authorization_pending"})))}
                        else {None};
                    if let Some((status,body)) = error {
                        let body = body.to_string();
                        return Reply::Raw(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()));
                    }
                }
                server::standard(request,3600)
            }).await;
            let fixture = Fixture::empty(remote, &server).await;
            let provider = provider(&fixture.client, Authentication::ChatGptOAuth).await;
            let (_, attempt) = start(&fixture.client, &provider).await;
            let mut updates = fixture.client.subscribe_login(attempt.id).await.unwrap();
            if outcome == "cancelled" {
                server.wait_requests(3).await;
                let request = fixture.client.prepare(Command::CancelProviderLogin {
                    attempt: attempt.id,
                });
                let first = fixture.client.execute(request.clone()).await.unwrap();
                assert_eq!(fixture.client.execute(request).await.unwrap(), first);
            }
            let update = terminal(&mut updates).await;
            no_secrets(&update);
            match outcome {
                "connected" => {
                    assert_eq!(update.state, login::State::Connected);
                    assert_eq!(polls.load(Ordering::SeqCst), 3);
                }
                "cancelled" => assert_eq!(update.state, login::State::Cancelled),
                "expired" => {
                    let login::State::Failed(error) = update.state else {
                        panic!("expiry expected")
                    };
                    assert_eq!(error.code, ErrorCode::Expired);
                    assert_eq!(polls.load(Ordering::SeqCst), 1);
                }
                "denied" => {
                    let login::State::Failed(error) = update.state else {
                        panic!("denial expected")
                    };
                    assert_eq!(error.code, ErrorCode::PermissionDenied);
                    assert_eq!(polls.load(Ordering::SeqCst), 1);
                }
                _ => unreachable!(),
            }
            if outcome != "connected" {
                assert!(
                    saved(&fixture.client, provider.id)
                        .await
                        .credential
                        .is_none()
                );
                assert!(
                    !server
                        .requests
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|request| request.path == "/oauth/token")
                );
            } else {
                assert_eq!(
                    server
                        .requests
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|request| request.path == "/oauth/token")
                        .count(),
                    1
                );
            }
            drop(updates);
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_invalid_identity_after_token_exchange() {
    for remote in [false, true] {
        let exchanges = Arc::new(AtomicUsize::new(0));
        let observed = exchanges.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/oauth/token" {
                observed.fetch_add(1, Ordering::SeqCst);
                let mut tokens = server::tokens(request, 3600, ACCESS, REFRESH);
                tokens["id_token"] = json!("PRIVATE_BROKEN_ID_TOKEN");
                return Reply::Json(tokens);
            }
            server::standard(request, 3600)
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = provider(&fixture.client, Authentication::ChatGptOAuth).await;
        let (_, attempt) = start(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        let update = terminal(&mut stream).await;
        no_secrets(&update);
        assert!(
            !serde_json::to_string(&update)
                .unwrap()
                .contains("PRIVATE_BROKEN_ID_TOKEN")
        );
        let login::State::Failed(error) = update.state else {
            panic!("invalid identity was accepted")
        };
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(error.message, "model authorization data is invalid");
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);
        assert_eq!(saved(&fixture.client, provider.id).await, provider);
        let Output::Credentials(credentials) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListCredentials))
            .await
            .unwrap()
        else {
            panic!("credentials expected")
        };
        assert!(credentials.is_empty());
        drop(stream);
        fixture.close().await;
    }
}
