use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn command(provider: &Provider) -> Command {
    Command::DiscoverModels(Box::new(discovery::Source::Saved {
        provider: provider.id,
        expected_revision: provider.revision,
    }))
}

pub(super) fn account_models(
    request: &discovery_support::Request,
    authentication: Authentication,
) -> serde_json::Value {
    if authentication == Authentication::ChatGpt && request.path != "/models?client_version=0.160.0"
    {
        return json!({"models": []});
    }
    models(authentication)
}

pub(super) fn models(authentication: Authentication) -> serde_json::Value {
    if authentication == Authentication::ChatGpt {
        json!({"models":[{
            "slug":"fixture", "context_window":4096, "input_modalities":["text","image"],
            "supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],
            "default_reasoning_level":"high"
        }, {"slug":"unknown"}]})
    } else {
        json!({"data":[{
            "id":"fixture", "supported_endpoints":["/responses","/chat/completions"],
            "capabilities":{"type":"chat", "limits":{"max_context_window_tokens":16384,"max_output_tokens":1024},
                "supports":{"vision":true,"tool_calls":true,"reasoning_effort":["low","high"]}}
        }, {"id":"unknown", "supported_endpoints":["/responses","/chat/completions"],"capabilities":{"type":"chat"}},
        {"id":"embedding", "capabilities":{"type":"embeddings"}},
        {"id":"messages-only", "supported_endpoints":["/v1/messages"],"capabilities":{"type":"chat"}}]})
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uses_compatibility_version() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, standard).await;
        let old: serde_json::Value = reqwest::Client::new()
            .get(format!("{}/models?client_version=0.1.0", server.endpoint))
            .header("chatgpt-account-id", "fixture-account")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(old["models"], json!([]));
        let fixture = Fixture::new(remote, &server).await;
        let provider = provider(&fixture.client, Authentication::ChatGpt).await;
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        let update = terminal(&mut stream).await;
        assert_eq!(update.state, login::State::Connected);
        assert_eq!(update.model_error, None);
        let Output::Snapshot(snapshot) = fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let saved = snapshot
            .providers
            .iter()
            .find(|saved| saved.id == provider.id)
            .unwrap();
        assert_eq!(saved.models.len(), 2);
        let Output::DiscoveredModels(catalog) = fixture
            .client
            .execute(fixture.client.prepare(command(saved)))
            .await
            .unwrap()
        else {
            panic!("catalog expected")
        };
        assert_eq!(catalog.models.len(), 2);
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.path == "/models?client_version=0.160.0")
                .count(),
            2
        );
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shares_refresh() {
    for remote in [false, true] {
        for authentication in [Authentication::ChatGpt, Authentication::Copilot] {
            let exchanges = Arc::new(AtomicUsize::new(0));
            let count = exchanges.clone();
            let release = Arc::new(tokio::sync::Notify::new());
            let gate = release.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if matches!(
                    request.path.as_str(),
                    "/oauth/token" | "/copilot_internal/v2/token"
                ) {
                    let fresh = count.fetch_add(1, Ordering::SeqCst) > 0;
                    if fresh {
                        if authentication == Authentication::ChatGpt {
                            assert!(request.body.contains("grant_type=refresh_token"));
                            assert!(request.body.contains(REFRESH));
                        } else {
                            assert_eq!(request.headers["authorization"], format!("token {GITHUB}"));
                        }
                        Reply::Delayed(gate.clone(), execution::exchanged(authentication, true))
                    } else {
                        Reply::Json(execution::exchanged(authentication, false))
                    }
                } else if request.path.starts_with("/models") {
                    if authentication == Authentication::Copilot {
                        assert_eq!(
                            request.headers["authorization"],
                            if count.load(Ordering::SeqCst) > 1 {
                                "Bearer renewed-access"
                            } else {
                                "Bearer isolated-copilot-secret"
                            }
                        );
                    }
                    Reply::Json(models(authentication))
                } else {
                    standard(request)
                }
            })
            .await;
            let fixture = Fixture::new(remote, &server).await;
            let provider =
                execution::signed_in(&fixture, authentication, ModelApi::Responses).await;
            let mut jobs = Vec::new();
            for _ in 0..4 {
                let client = fixture.client.clone();
                let request = client.prepare(command(&provider));
                jobs.push(tokio::spawn(async move { client.execute(request).await }));
            }
            server.wait_requests(5).await;
            assert_eq!(exchanges.load(Ordering::SeqCst), 2);
            release.notify_one();
            for job in jobs {
                let output = job.await.unwrap().unwrap();
                no_secrets(&output);
                assert!(matches!(output, Output::DiscoveredModels(_)));
            }
            assert_eq!(exchanges.load(Ordering::SeqCst), 2);
            let Output::Credentials(credentials) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListCredentials))
                .await
                .unwrap()
            else {
                panic!("credentials expected")
            };
            assert_eq!(
                credentials[0].revision,
                if authentication == Authentication::ChatGpt {
                    3
                } else {
                    2
                }
            );
            fixture.close().await;
        }
    }
}

fn catalog(request: &discovery_support::Request, authentication: Authentication) -> Reply {
    assert_eq!(request.method, "GET");
    assert!(request.body.is_empty());
    assert_eq!(request.headers["user-agent"], "Sailry/0.1");
    if authentication == Authentication::ChatGpt {
        assert_eq!(request.path, "/models?client_version=0.160.0");
        assert!(request.headers["authorization"].starts_with("Bearer e30."));
        assert_eq!(request.headers["chatgpt-account-id"], "fixture-account");
        assert_eq!(request.headers["originator"], "sailry");
    } else {
        assert_eq!(request.path, "/models");
        assert_eq!(request.headers["authorization"], format!("Bearer {ACCESS}"));
        assert_eq!(request.headers["openai-intent"], "conversation-panel");
        assert_eq!(request.headers["x-initiator"], "user");
        assert_eq!(request.headers["copilot-integration-id"], "vscode-chat");
        assert!(request.headers["x-request-id"].parse::<RequestId>().is_ok());
    }
    Reply::Json(models(authentication))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reads_native_metadata() {
    for remote in [false, true] {
        for (authentication, api) in [
            (Authentication::ChatGpt, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::ChatCompletions),
        ] {
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path.starts_with("/models") {
                    catalog(request, authentication)
                } else {
                    standard(request)
                }
            })
            .await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = execution::signed_in(&fixture, authentication, api).await;
            let before = fixture
                .client
                .execute(fixture.client.prepare(Command::Snapshot))
                .await
                .unwrap();
            let admission = fixture
                .client
                .dispatch(fixture.client.prepare(command(&provider)))
                .await
                .unwrap();
            assert!(!admission.receipt.durable);
            let output = admission.completion.await.unwrap().unwrap();
            no_secrets(&output);
            let Output::DiscoveredModels(discovered) = output else {
                panic!("models expected")
            };
            let discovered = discovered.models;
            assert_eq!(discovered.len(), 2);
            assert_eq!(discovered[0].id, "fixture");
            assert_eq!(discovered[1].id, "unknown");
            assert_eq!(discovered[1].context, None);
            assert_eq!(discovered[1].output, None);
            let capabilities = discovered[0].capabilities.as_ref().unwrap();
            assert_eq!(capabilities.vision, Some(true));
            assert_eq!(capabilities.efforts, Some(vec![Effort::Low, Effort::High]));
            assert_eq!(
                capabilities.default_effort,
                (authentication == Authentication::ChatGpt).then_some(Effort::High)
            );
            let Output::ProviderValidation(validation) = fixture
                .client
                .execute(fixture.client.prepare(Command::ValidateProvider {
                    provider: provider.id,
                    expected_revision: provider.revision,
                }))
                .await
                .unwrap()
            else {
                panic!("validation expected")
            };
            assert!(validation.missing.is_empty());
            assert_eq!(
                validation.exceeded,
                if authentication == Authentication::ChatGpt {
                    vec!["fixture"]
                } else {
                    vec![]
                }
            );
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::Snapshot))
                    .await
                    .unwrap(),
                before
            );
            assert_eq!(
                server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| request.path.starts_with("/models"))
                    .count(),
                3
            );
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_changed_accounts() {
    for remote in [false, true] {
        for mutation in ["provider", "revoke", "reconnect"] {
            let release = Arc::new(tokio::sync::Notify::new());
            let gate = release.clone();
            let queries = Arc::new(AtomicUsize::new(0));
            let counter = queries.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path.starts_with("/models") {
                    if counter.fetch_add(1, Ordering::SeqCst) == 1 {
                        Reply::Delayed(gate.clone(), models(Authentication::Copilot))
                    } else {
                        Reply::Json(models(Authentication::Copilot))
                    }
                } else {
                    standard(request)
                }
            })
            .await;
            let fixture = Fixture::new(remote, &server).await;
            let provider =
                execution::signed_in(&fixture, Authentication::Copilot, ModelApi::Responses).await;
            let client = fixture.client.clone();
            let request = client.prepare(command(&provider));
            let query = tokio::spawn(async move { client.execute(request).await });
            server.wait_requests(5).await;
            assert_eq!(queries.load(Ordering::SeqCst), 2);
            match mutation {
                "revoke" => {
                    fixture
                        .client
                        .execute(fixture.client.prepare(Command::RevokeCredential {
                            id: provider.credential.as_ref().unwrap().id,
                            expected_revision: 1,
                        }))
                        .await
                        .unwrap();
                }
                "reconnect" => {
                    let (_, attempt) = begin(&fixture.client, &provider).await;
                    let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
                    assert_eq!(terminal(&mut stream).await.state, login::State::Connected);
                }
                _ => {
                    let mut changed = provider.clone();
                    changed.name = "Changed during discovery".into();
                    fixture
                        .client
                        .execute(fixture.client.prepare(Command::PutProvider {
                            expected_revision: changed.revision,
                            provider: changed,
                        }))
                        .await
                        .unwrap();
                }
            }
            release.notify_one();
            let error = query.await.unwrap().unwrap_err();
            assert_eq!(
                error.code,
                if mutation == "revoke" {
                    ErrorCode::PermissionDenied
                } else {
                    ErrorCode::RevisionConflict
                }
            );
            no_secrets(&error);
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounds_and_cancels_queries() {
    for remote in [false, true] {
        let queries = AtomicUsize::new(0);
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path.starts_with("/models") && queries.fetch_add(1, Ordering::SeqCst) > 0 {
                Reply::Hold
            } else {
                standard(request)
            }
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let provider =
            execution::signed_in(&fixture, Authentication::Copilot, ModelApi::Responses).await;
        let mut jobs = Vec::new();
        for _ in 0..4 {
            let client = fixture.client.clone();
            let request = client.prepare(command(&provider));
            jobs.push(tokio::spawn(async move { client.execute(request).await }));
        }
        server.wait_requests(8).await;
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(command(&provider)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap();
        for job in jobs {
            job.abort();
            let _ = job.await;
        }
        server.wait_closed(4).await;
        let client = fixture.client.clone();
        let request = client.prepare(command(&provider));
        let query = tokio::spawn(async move { client.execute(request).await });
        server.wait_requests(9).await;
        tokio::time::timeout(Duration::from_secs(5), fixture.node.shutdown())
            .await
            .unwrap()
            .unwrap();
        assert!(query.await.unwrap().is_err());
        server.wait_closed(5).await;
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_unsafe_responses() {
    for remote in [false, true] {
        let target = Server::start(ModelApi::Responses, |_| Reply::Json(json!({"data":[]}))).await;
        for (response, code) in [
            (
                format!(
                    "HTTP/1.1 307 Temporary Redirect\r\nLocation: {}/models\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    target.endpoint
                ),
                ErrorCode::Unavailable,
            ),
            (
                format!(
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{ACCESS}",
                    ACCESS.len()
                ),
                ErrorCode::NotConfigured,
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 2097153\r\nConnection: close\r\n\r\nx".into(),
                ErrorCode::Unavailable,
            ),
        ] {
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path.starts_with("/models") {
                    Reply::Raw(response.clone())
                } else {
                    standard(request)
                }
            })
            .await;
            let fixture = Fixture::new(remote, &server).await;
            let provider =
                execution::signed_in(&fixture, Authentication::Copilot, ModelApi::Responses).await;
            let error = fixture
                .client
                .execute(fixture.client.prepare(command(&provider)))
                .await
                .unwrap_err();
            assert_eq!(error.code, code);
            no_secrets(&error);
            assert_eq!(server.requests.lock().unwrap().len(), 5);
            fixture.close().await;
        }
        assert!(target.requests.lock().unwrap().is_empty());
    }
}
