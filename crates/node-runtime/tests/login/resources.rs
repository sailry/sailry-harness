//! Native resources use real fixture logins and the ordinary local/remote command path.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

async fn installed(fixture: &Fixture, authentication: Authentication) -> plugin::Info {
    installed_with_cache(fixture, authentication, None).await
}

async fn installed_with_cache(
    fixture: &Fixture,
    authentication: Authentication,
    ttl: Option<u32>,
) -> plugin::Info {
    let source = fixture.directory.path().join("resources");
    std::fs::create_dir_all(source.join("package")).unwrap();
    let origin = if authentication == Authentication::ClaudeOAuth {
        "https://api.anthropic.com"
    } else {
        "https://chatgpt.com"
    };
    let mut manifest = json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"example","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["http.request"],"model_resources":[{"authentication":authentication,"requests":{
        "usage":{"method":"GET","url":format!("{origin}/usage"),"headers":{"Accept":"application/json","User-Agent":"Resource-Fixture/1","anthropic-beta":"oauth-2025-04-20"}},
        "foreign":{"method":"GET","url":"https://example.invalid/usage"},
        "override":{"method":"GET","url":format!("{origin}/usage"),"headers":{"chatgpt-account-id":"caller-account"}}
    }}]}}});
    if let Some(ttl) = ttl {
        let resource =
            &mut manifest["extensions"]["dev.sailry.platform"]["model_resources"][0]["requests"];
        resource["usage"]["cache_ttl_ms"] = json!(ttl);
        resource["uncached"] = resource["usage"].clone();
        resource["uncached"]
            .as_object_mut()
            .unwrap()
            .remove("cache_ttl_ms");
    }
    std::fs::write(source.join("package/plugin.json"), manifest.to_string()).unwrap();
    let mut foreign = manifest.clone();
    foreign["name"] = json!("foreign");
    foreign["extensions"]["dev.sailry.platform"]["model_resources"] = json!([]);
    std::fs::create_dir_all(source.join("foreign")).unwrap();
    std::fs::write(source.join("foreign/plugin.json"), foreign.to_string()).unwrap();
    package::support::execute(
        &fixture.client,
        Command::RegisterProject {
            name: "Resource fixture".into(),
            path: source.to_str().unwrap().into(),
        },
    )
    .await;
    let Output::Snapshot(snapshot) =
        package::support::execute(&fixture.client, Command::Snapshot).await
    else {
        panic!("snapshot expected")
    };
    let info = package::support::info(
        package::support::execute(
            &fixture.client,
            package::support::install(snapshot.worktrees[0].id, 0),
        )
        .await,
    );
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    let foreign = package::support::info(
        package::support::execute(
            &fixture.client,
            Command::InstallPlugin {
                worktree: snapshot.worktrees[0].id,
                path: "foreign".into(),
                name: "foreign".into(),
                expected_revision: 0,
                expected_digest: None,
            },
        )
        .await,
    );
    assert!(foreign.issues.is_empty(), "{:?}", foreign.issues);
    info
}

fn context(package: plugin::Reference) -> plugin::Context {
    plugin::Context {
        package,
        worktree: None,
        session: None,
        turn: None,
        invocation: None,
        surface: Default::default(),
    }
}

fn request(
    client: &Client,
    provider: &Provider,
    package: &plugin::Reference,
    name: &str,
) -> Request {
    client
        .prepare(Command::RequestProviderHttp {
            provider: provider.id,
            expected_revision: provider.revision,
            request: name.into(),
            input: Default::default(),
        })
        .with_plugin(context(package.clone()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protects_accounts_across_transports() {
    for remote in [false, true] {
        for authentication in [
            Authentication::ChatGpt,
            Authentication::ChatGptOAuth,
            Authentication::ClaudeOAuth,
        ] {
            let mode = Arc::new(AtomicUsize::new(0));
            let selected = mode.clone();
            let exchanges = Arc::new(AtomicUsize::new(0));
            let counter = exchanges.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path == "/usage" {
                    assert_eq!(request.method, "GET");
                    assert!(request.body.is_empty());
                    assert_eq!(request.headers["user-agent"], "Resource-Fixture/1");
                    assert!(!request.headers.contains_key("originator"));
                    assert!(!request.headers.contains_key("anthropic-version"));
                    let access = match authentication {
                        Authentication::ChatGpt => {
                            assert!(!request.headers.contains_key("chatgpt-account-id"));
                            "isolated-chatgpt-access"
                        }
                        Authentication::ChatGptOAuth => {
                            assert_eq!(request.headers["chatgpt-account-id"], codex::ACCOUNT);
                            codex::REFRESHED
                        }
                        Authentication::ClaudeOAuth => {
                            assert_eq!(request.headers["anthropic-beta"], "oauth-2025-04-20");
                            assert!(!request.headers.contains_key("chatgpt-account-id"));
                            "isolated-claude-renewed-access"
                        }
                        _ => unreachable!(),
                    };
                    assert_eq!(request.headers["authorization"], format!("Bearer {access}"));
                    return match selected.load(Ordering::SeqCst) {
                        0 => Reply::Json(json!({"used_percent":0,"reset_at":null,"account_id":request.headers.get("chatgpt-account-id")})),
                        1 => Reply::Json(json!({"echo":access})),
                        _ => Reply::Raw("HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nRetry-After: 7\r\nX-Private: hidden\r\nConnection: close\r\n\r\n{\"message\":\"Usage is temporarily unavailable\"}".into()),
                    };
                }
                match authentication {
                    Authentication::ChatGpt => {
                        if request.path == "/api/accounts/oauth/token" {
                            let fresh = counter.fetch_add(1, Ordering::SeqCst) > 0;
                            Reply::Json(identity::tokens(request, if fresh {3600} else {30}, REFRESH))
                        } else { standard(request) }
                    }
                    Authentication::ChatGptOAuth => {
                        if request.path == "/oauth/token" {
                            let fresh = counter.fetch_add(1, Ordering::SeqCst) > 0;
                            Reply::Json(codex::server::tokens(request, if fresh {3600} else {30}, if fresh {codex::REFRESHED} else {codex::ACCESS}, if fresh {codex::ROTATED} else {codex::REFRESH}))
                        } else { codex::server::standard(request, 30) }
                    }
                    Authentication::ClaudeOAuth => {
                        if request.path == "/v1/oauth/token" {
                            let fresh = counter.fetch_add(1, Ordering::SeqCst) > 0;
                            Reply::Json(claude::server::tokens(request, fresh, if fresh {3600} else {330}))
                        } else { claude::server::standard(request) }
                    }
                    _ => unreachable!(),
                }
            }).await;
            let fixture = Fixture::empty(remote, &server).await;
            let mut provider = match authentication {
                Authentication::ChatGpt => {
                    execution::signed_in(&fixture, authentication, ModelApi::Responses).await
                }
                Authentication::ChatGptOAuth => codex::configured(&fixture, None).await,
                Authentication::ClaudeOAuth => claude::connected(&fixture).await,
                _ => unreachable!(),
            };
            let installed = installed(&fixture, authentication).await;
            let reference = installed.summary.reference();
            let prepared = request(&fixture.client, &provider, &reference, "usage");
            assert!(!prepared.command.durable());
            let id = prepared.id;
            let admission = fixture.client.dispatch(prepared).await.unwrap();
            assert!(!admission.receipt.durable);
            let Output::PluginHttp(response) = admission.completion.await.unwrap().unwrap() else {
                panic!("HTTP response expected")
            };
            assert_eq!(response.status, 200);
            let mut data: serde_json::Value = serde_json::from_str(&response.body).unwrap();
            assert_eq!(
                data,
                json!({"used_percent":0,"reset_at":null,"account_id":if authentication == Authentication::ChatGptOAuth {Some(codex::ACCOUNT)} else {None}})
            );
            // Only the asserted public identity field is excluded from this older
            // fixture helper, which intentionally treats account metadata as secret.
            data.as_object_mut().unwrap().remove("account_id");
            no_secrets(&data);
            no_secrets(&response.headers);
            assert_eq!(
                exchanges.load(Ordering::SeqCst),
                2,
                "resource reads reuse the normal refresh owner"
            );
            if authentication == Authentication::ChatGpt {
                let previous = provider.clone();
                let (_, attempt) = begin(&fixture.client, &provider).await;
                let mut updates = fixture.client.subscribe_login(attempt.id).await.unwrap();
                assert_eq!(terminal(&mut updates).await.state, login::State::Connected);
                let Output::Providers(providers) =
                    package::support::execute(&fixture.client, Command::ListProviders).await
                else {
                    panic!("providers expected")
                };
                provider = providers
                    .into_iter()
                    .find(|value| value.id == previous.id)
                    .unwrap();
                assert!(provider.revision > previous.revision);
                assert_eq!(provider.credential, previous.credential);
                assert_eq!(
                    fixture
                        .client
                        .execute(request(&fixture.client, &previous, &reference, "usage"))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::RevisionConflict
                );
            }
            let count = server.requests.lock().unwrap().len();
            for (name, expected) in [
                ("missing", ErrorCode::NotConfigured),
                ("foreign", ErrorCode::PermissionDenied),
                ("override", ErrorCode::InvalidRequest),
            ] {
                assert_eq!(
                    fixture
                        .client
                        .execute(request(&fixture.client, &provider, &reference, name))
                        .await
                        .unwrap_err()
                        .code,
                    expected
                );
            }
            let mut stale = provider.clone();
            stale.revision += 1;
            assert_eq!(
                fixture
                    .client
                    .execute(request(&fixture.client, &stale, &reference, "usage"))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::RevisionConflict
            );
            let Output::Plugin(foreign) = package::support::execute(
                &fixture.client,
                Command::ReadPlugin {
                    name: "foreign".into(),
                },
            )
            .await
            else {
                panic!("plugin expected")
            };
            assert_eq!(
                fixture
                    .client
                    .execute(request(
                        &fixture.client,
                        &provider,
                        &foreign.summary.reference(),
                        "usage"
                    ))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
            let mutation = fixture
                .client
                .prepare(Command::ExecuteProviderHttp {
                    provider: provider.id,
                    expected_revision: provider.revision,
                    request: "usage".into(),
                    input: Default::default(),
                })
                .with_plugin(context(reference.clone()));
            assert_eq!(
                fixture.client.execute(mutation).await.unwrap_err().code,
                ErrorCode::PermissionDenied
            );
            assert_eq!(server.requests.lock().unwrap().len(), count);
            mode.store(1, Ordering::SeqCst);
            let error = fixture
                .client
                .execute(request(&fixture.client, &provider, &reference, "usage"))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::PermissionDenied);
            no_secrets(&error);
            mode.store(2, Ordering::SeqCst);
            let Output::PluginHttp(response) = fixture
                .client
                .execute(request(&fixture.client, &provider, &reference, "usage"))
                .await
                .unwrap()
            else {
                panic!("HTTP response expected")
            };
            assert_eq!(response.status, 429);
            assert_eq!(response.headers["retry-after"], ["7"]);
            assert!(!response.headers.contains_key("x-private"));
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&response.body).unwrap()["message"],
                "Usage is temporarily unavailable"
            );
            assert_eq!(
                server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r.path == "/usage")
                    .count(),
                3
            );
            let count = server.requests.lock().unwrap().len();
            let Output::Credentials(credentials) =
                package::support::execute(&fixture.client, Command::ListCredentials).await
            else {
                panic!("credentials expected")
            };
            let credential = credentials
                .into_iter()
                .find(|value| Some(value.id) == provider.credential.as_ref().map(|value| value.id))
                .unwrap();
            package::support::execute(
                &fixture.client,
                Command::RevokeCredential {
                    id: credential.id,
                    expected_revision: credential.revision,
                },
            )
            .await;
            assert!(
                fixture
                    .client
                    .execute(request(&fixture.client, &provider, &reference, "usage"))
                    .await
                    .is_err()
            );
            assert_eq!(server.requests.lock().unwrap().len(), count);
            let db = rusqlite::Connection::open_with_flags(
                fixture.node.profile().join("storage/node.sqlite3"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM requests WHERE id=?1",
                    [id.to_string()],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            drop(db);
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shares_reads_across_sessions_and_clients() {
    let reads = Arc::new(AtomicUsize::new(0));
    let observed = reads.clone();
    let release = Arc::new(tokio::sync::Notify::new());
    let held = release.clone();
    let entered = Arc::new(tokio::sync::Notify::new());
    let starting = entered.clone();
    let server = Server::start_with_request(ModelApi::Responses, move |request| {
        if request.path == "/usage" {
            assert_eq!(
                request.headers["authorization"],
                format!("Bearer {}", codex::ACCESS)
            );
            let index = observed.fetch_add(1, Ordering::SeqCst);
            let body = json!({"used_percent":index});
            if index == 0 {
                starting.notify_one();
                return Reply::Delayed(held.clone(), body);
            }
            return Reply::Json(body);
        }
        codex::server::standard(request, 3600)
    })
    .await;
    let fixture = Fixture::empty(false, &server).await;
    let provider = codex::configured(&fixture, None).await;
    let installed = installed_with_cache(&fixture, Authentication::ChatGptOAuth, Some(60000)).await;
    let package = installed.summary.reference();
    let mut sessions = Vec::new();
    for _ in 0..2 {
        let Output::Session(session) = package::support::execute(
            &fixture.client,
            Command::CreateSession {
                project: None,
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: provider.id,
                    model: provider.default_model.clone(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    options: Default::default(),
                    permission: Permission::Ask,
                    credential: provider.credential.clone(),
                }),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        sessions.push(session);
    }
    let remote = Client::new(
        fixture
            .controller
            .handle()
            .remote(fixture.node.link().address()),
    );
    let mut first = request(&fixture.client, &provider, &package, "usage");
    first.plugin.as_mut().unwrap().session = Some(sessions[0].id);
    first.plugin.as_mut().unwrap().worktree = Some(sessions[0].worktree);
    let mut second = request(&remote, &provider, &package, "usage");
    second.plugin.as_mut().unwrap().session = Some(sessions[1].id);
    second.plugin.as_mut().unwrap().worktree = Some(sessions[1].worktree);
    let (first, second, ()) = tokio::join!(
        async {
            let result = fixture.client.execute(first).await;
            assert!(
                result.is_ok(),
                "local resource failed: {:?}",
                result.as_ref().err().map(|error| error.code)
            );
            result
        },
        async {
            let result = remote.execute(second).await;
            assert!(
                result.is_ok(),
                "remote resource failed: {:?}",
                result.as_ref().err().map(|error| error.code)
            );
            result
        },
        async {
            entered.notified().await;
            release.notify_one();
        }
    );
    assert_eq!(first.unwrap(), second.unwrap());
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    fixture
        .client
        .execute(request(&fixture.client, &provider, &package, "usage"))
        .await
        .unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    for client in [&*fixture.client, &remote] {
        client
            .execute(request(client, &provider, &package, "uncached"))
            .await
            .unwrap();
    }
    assert_eq!(reads.load(Ordering::SeqCst), 3);
    let (_, attempt) = begin(&fixture.client, &provider).await;
    let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
    assert_eq!(terminal(&mut stream).await.state, login::State::Connected);
    let Output::Providers(providers) =
        package::support::execute(&fixture.client, Command::ListProviders).await
    else {
        panic!("providers expected")
    };
    let current = providers
        .into_iter()
        .find(|current| current.id == provider.id)
        .unwrap();
    assert_eq!(
        fixture
            .client
            .execute(request(&fixture.client, &provider, &package, "usage"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    fixture
        .client
        .execute(request(&fixture.client, &current, &package, "usage"))
        .await
        .unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 4);
    fixture.close().await;
}
