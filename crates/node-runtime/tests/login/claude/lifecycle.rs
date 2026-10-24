use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connects_and_restores_both_callback_flows() {
    for remote in [false, true] {
        for redirect in [Some(REDIRECT), None] {
            let agent = if redirect.is_some() {
                "claude-cli/2.1.280"
            } else {
                "claude-fixture/copy"
            };
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                assert_eq!(request.headers["user-agent"], agent);
                if request.path.starts_with("/v1/models") {
                    server::catalog(request, ACCESS, agent)
                } else {
                    server::standard(request)
                }
            })
            .await;
            let fixture = Fixture::empty(remote, &server).await;
            let original = provider(
                &fixture.client,
                redirect.is_none().then(|| Options::ClaudeOAuth {
                    user_agent: agent.into(),
                }),
            )
            .await;
            let (request, attempt, url) = begin(&fixture.client, &original, redirect).await;
            server::authorization(&url, &server.endpoint, redirect);
            let callback = callback(&url, redirect);
            assert!(!format!("{callback:?}").contains(CODE));
            let completed = super::super::complete(&fixture.client, &attempt, callback).await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            let update = super::super::terminal(&mut stream).await;
            redacted(&update);
            assert_eq!(update.state, login::State::Connected);
            assert_eq!(update.model_error, None);
            let connected = saved(&fixture.client, original.id).await;
            assert_eq!(connected.api, ModelApi::Anthropic);
            assert_eq!(connected.authentication, Authentication::ClaudeOAuth);
            assert_eq!(connected.endpoint, original.endpoint);
            assert_eq!(connected.oauth, original.oauth);
            assert_eq!(connected.revision, original.revision + 1);
            assert_eq!(connected.default_model, "fixture");
            assert_eq!(
                connected
                    .models
                    .iter()
                    .map(|model| model.id.as_str())
                    .collect::<Vec<_>>(),
                ["fixture", "unknown"]
            );
            assert_eq!(
                (connected.models[0].context, connected.models[0].output),
                (8192, 512)
            );
            assert!(connected.models[0].tools && connected.models[0].vision);
            assert_eq!(
                (connected.models[1].context, connected.models[1].output),
                (16384, 1024)
            );
            let private = grant(&fixture, &connected).await;
            assert!(private["access"] == ACCESS && private["refresh"] == REFRESH);
            assert_eq!(private["kind"], "claude_oauth");
            assert_eq!(private["renewal_pending"], false);
            let remaining = private["expires_at_ms"]
                .as_u64()
                .unwrap()
                .saturating_sub((future_expiry() - 3600) * 1000);
            assert!((3_240_000..=3_301_000).contains(&remaining));
            let Output::Credentials(credentials) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListCredentials))
                .await
                .unwrap()
            else {
                panic!("credentials expected")
            };
            redacted(&credentials);
            assert_eq!(credentials.len(), 1);
            assert_eq!(credentials[0].authentication, Authentication::ClaudeOAuth);
            assert_eq!(credentials[0].expires_at_ms, None);
            assert_eq!(credentials[0].account_label, None);
            let error = fixture
                .client
                .execute(fixture.client.prepare(Command::ReadProviderKey {
                    provider: connected.id,
                    expected_revision: connected.revision,
                }))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::PermissionDenied);
            redacted(&error);
            let error = fixture
                .node
                .resolve_credential(connected.credential.clone().unwrap(), connected.id)
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::PermissionDenied);
            redacted(&error);
            redacted(
                &fixture
                    .client
                    .execute(fixture.client.prepare(Command::Snapshot))
                    .await
                    .unwrap(),
            );
            public_rows(&fixture, &url).await;
            assert_eq!(server.requests.lock().unwrap().len(), 3);
            {
                let authorization = url::Url::parse(&url).unwrap();
                let state = authorization
                    .query_pairs()
                    .find(|(key, _)| key == "state")
                    .unwrap()
                    .1
                    .into_owned();
                let requests = server.requests.lock().unwrap();
                let body: Value = serde_json::from_str(&requests[0].body).unwrap();
                assert!(body["state"].as_str() == Some(state.as_str()));
                assert!(body["code_verifier"].as_str() == Some(state.as_str()));
            }
            assert_eq!(
                fixture.client.execute(request.clone()).await.unwrap(),
                Output::ProviderLogin(attempt.clone())
            );
            assert_eq!(
                fixture.client.execute(completed.clone()).await.unwrap(),
                Output::ProviderLogin(attempt.clone())
            );
            drop(stream);
            let fixture =
                super::super::execution::renewals::restart(fixture, &server, remote).await;
            assert_eq!(saved(&fixture.client, connected.id).await, connected);
            assert!(grant(&fixture, &connected).await["access"] == ACCESS);
            assert_eq!(
                fixture.client.execute(request).await.unwrap(),
                Output::ProviderLogin(attempt.clone())
            );
            assert_eq!(
                fixture.client.execute(completed).await.unwrap(),
                Output::ProviderLogin(attempt.clone())
            );
            assert_eq!(
                super::super::subscription_error(fixture.client.subscribe_login(attempt.id).await)
                    .await
                    .code,
                ErrorCode::NotFound
            );
            assert_eq!(server.requests.lock().unwrap().len(), 3);
            public_rows(&fixture, &url).await;
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_callbacks_before_token_dispatch() {
    for remote in [false, true] {
        for invalid in ["state", "redirect", "duplicate", "denied"] {
            let server = Server::start_with_request(ModelApi::Anthropic, |_| {
                panic!("invalid callback must not dispatch HTTP")
            })
            .await;
            let fixture = Fixture::empty(remote, &server).await;
            let original = provider(&fixture.client, None).await;
            let (_, attempt, url) = begin(&fixture.client, &original, Some(REDIRECT)).await;
            let callback = match invalid {
                "state" => Secret::new(format!("{CODE}#wrong-state")),
                "redirect" => Secret::new(format!("http://127.0.0.1:53693/callback?code={CODE}")),
                "duplicate" => Secret::new(format!("{REDIRECT}?code={CODE}&code=duplicate")),
                "denied" => Secret::new(format!("{REDIRECT}?error=access_denied")),
                _ => unreachable!(),
            };
            super::super::complete(&fixture.client, &attempt, callback).await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            let update = super::super::terminal(&mut stream).await;
            redacted(&update);
            let login::State::Failed(error) = update.state else {
                panic!("invalid callback failure expected")
            };
            assert_eq!(
                error.code,
                if invalid == "denied" {
                    ErrorCode::PermissionDenied
                } else {
                    ErrorCode::InvalidRequest
                }
            );
            assert_eq!(saved(&fixture.client, original.id).await, original);
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ListCredentials))
                    .await
                    .unwrap(),
                Output::Credentials(vec![])
            );
            assert!(server.requests.lock().unwrap().is_empty());
            public_rows(&fixture, &url).await;
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancels_and_rejects_late_provider_changes() {
    for remote in [false, true] {
        for mode in ["pending", "exchanging", "conflict"] {
            let release = Arc::new(tokio::sync::Notify::new());
            let gate = release.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path == "/v1/oauth/token" {
                    if mode == "exchanging" {
                        Reply::Hold
                    } else {
                        Reply::Delayed(gate.clone(), server::tokens(request, false, 3600))
                    }
                } else {
                    server::standard(request)
                }
            })
            .await;
            let fixture = Fixture::empty(remote, &server).await;
            let original = provider(&fixture.client, None).await;
            let (_, attempt, url) = begin(&fixture.client, &original, Some(REDIRECT)).await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            let mut expected = original.clone();
            if mode != "pending" {
                super::super::complete(&fixture.client, &attempt, callback(&url, Some(REDIRECT)))
                    .await;
                server.wait_requests(1).await;
            }
            if mode == "conflict" {
                expected.name = "Edited during Claude login".into();
                let Output::Provider(saved) = fixture
                    .client
                    .execute(fixture.client.prepare(Command::PutProvider {
                        expected_revision: expected.revision,
                        provider: expected,
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("provider expected")
                };
                expected = saved;
                release.notify_one();
            } else {
                let cancel = fixture.client.prepare(Command::CancelProviderLogin {
                    attempt: attempt.id,
                });
                let result = fixture.client.execute(cancel.clone()).await.unwrap();
                assert_eq!(fixture.client.execute(cancel).await.unwrap(), result);
            }
            let update = super::super::terminal(&mut stream).await;
            redacted(&update);
            if mode == "conflict" {
                let login::State::Failed(error) = update.state else {
                    panic!("revision conflict expected")
                };
                assert_eq!(error.code, ErrorCode::RevisionConflict);
            } else {
                assert_eq!(update.state, login::State::Cancelled);
                if mode == "exchanging" {
                    server.wait_closed(1).await;
                } else {
                    assert!(server.requests.lock().unwrap().is_empty());
                }
            }
            assert_eq!(saved(&fixture.client, original.id).await, expected);
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ListCredentials))
                    .await
                    .unwrap(),
                Output::Credentials(vec![])
            );
            public_rows(&fixture, &url).await;
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_denials_and_expired_grants() {
    for remote in [false, true] {
        for expiry in [0, 300, 3600] {
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                assert_eq!(request.path, "/v1/oauth/token");
                if expiry == 3600 {
                    Reply::Raw(format!("HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{ACCESS}", ACCESS.len()))
                } else { Reply::Json(server::tokens(request, false, expiry)) }
            }).await;
            let fixture = Fixture::empty(remote, &server).await;
            let original = provider(&fixture.client, None).await;
            let (_, attempt, url) = begin(&fixture.client, &original, None).await;
            super::super::complete(&fixture.client, &attempt, callback(&url, None)).await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            let update = super::super::terminal(&mut stream).await;
            assert!(matches!(update.state, login::State::Failed(_)));
            redacted(&update);
            assert_eq!(saved(&fixture.client, original.id).await, original);
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ListCredentials))
                    .await
                    .unwrap(),
                Output::Credentials(vec![])
            );
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            public_rows(&fixture, &url).await;
            fixture.close().await;
        }
    }
}
