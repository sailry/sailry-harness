use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discovers_with_private_authorization() {
    for remote in [false, true] {
        let status = Arc::new(AtomicUsize::new(200));
        let returned = status.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            match request.path.as_str() {
                "/token" => tokens(true),
                "/catalog" => {
                    assert_eq!(request.headers["authorization"], format!("Bearer {FRESH}"));
                    assert_eq!(request.headers["x-account"], ACCOUNT);
                    assert_eq!(request.headers["x-catalog"], "declared");
                    match returned.load(Ordering::SeqCst) {
                        200 => Reply::Json(json!({"catalog":[{"id":"translated","context":64000,"output":4096,"capabilities":{"vision":true}}]})),
                        value => {
                            let body=json!({"error":{"message":ACCOUNT}}).to_string();
                            Reply::Raw(format!("HTTP/1.1 {value} Synthetic\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()))
                        },
                    }
                }
                "/generate" => {
                    assert_eq!(request.headers["authorization"],format!("Bearer {FRESH}"));
                    assert_eq!(request.headers["x-account"],ACCOUNT);
                    let input:serde_json::Value=serde_json::from_str(&request.body).unwrap();private(&input);
                    if request.body.contains("Hold the parent") {Reply::Hold}
                    else {Reply::Json(json!({"content":"Translated response"}))}
                }
                _ => panic!("unexpected discovery route"),
            }
        }).await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = configured_channel(&fixture, true, json!({"kind":"token_exchange","token":{"url":"https://auth.example.test/token","method":"POST"}}), Some(&server.endpoint)).await;
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(models(&provider)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert!(server.requests.lock().unwrap().is_empty());
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        loop {
            let update = next(&mut stream).await;
            private(&update);
            match update.state {
                login::State::Starting | login::State::Exchanging => {}
                login::State::Connected => {
                    assert_eq!(update.model_error, None);
                    break;
                }
                state => panic!("unexpected adapter login state: {state:?}"),
            }
        }
        let provider = saved(&fixture.client, provider.id).await;
        assert!(provider.credential.is_some());
        assert_eq!(provider.models[0].id, "translated");
        assert_eq!(credentials(&fixture.client).await.len(), 1);
        let Output::DiscoveredModels(catalog) = fixture
            .client
            .execute(fixture.client.prepare(models(&provider)))
            .await
            .unwrap()
        else {
            panic!("catalog expected")
        };
        private(&catalog);
        assert_eq!(catalog.models[0].id, "translated");
        assert_eq!(catalog.models[0].context, Some(64000));
        let provider = exercise_model_services(&fixture, provider.clone(), &server).await;
        for status_code in [401, 403, 503] {
            status.store(status_code, Ordering::SeqCst);
            let error = fixture
                .client
                .execute(fixture.client.prepare(models(&provider)))
                .await
                .unwrap_err();
            private(&error);
            assert_eq!(error.code, ErrorCode::Unavailable);
            assert_eq!(
                error.message,
                format!("model discovery failed with HTTP {status_code}")
            );
        }
        assert_eq!(
            saved(&fixture.client, provider.id).await.credential,
            provider.credential
        );
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.path == "/catalog")
                .count(),
            5
        );
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.path == "/token")
                .count(),
            1
        );
        fixture.close().await;
    }
}

async fn exercise_model_services(
    fixture: &Fixture,
    mut provider: Provider,
    server: &Server,
) -> Provider {
    use support::execute;
    let Output::Plugin(info) = execute(
        &fixture.client,
        Command::ReadPlugin {
            name: "example".into(),
        },
    )
    .await
    else {
        panic!("plugin expected")
    };
    let mut context = plugin::Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: info.summary.reference(),
        worktree: None,
        session: None,
    };
    let text = || Command::GeneratePluginText {
        prompt: "Translate this request".into(),
        model: format!("{}/translated", provider.id),
        effort: None,
    };
    assert_eq!(
        fixture
            .client
            .execute(fixture.client.prepare(text()).with_plugin(context.clone()))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    provider.consent.as_mut().unwrap().scopes.messages = true;
    let Output::Provider(provider) = execute(
        &fixture.client,
        Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    let Output::PluginText(output) = fixture
        .client
        .execute(
            fixture
                .client
                .prepare(Command::GeneratePluginText {
                    prompt: "Translate this request".into(),
                    model: format!("{}/translated", provider.id),
                    effort: None,
                })
                .with_plugin(context.clone()),
        )
        .await
        .unwrap()
    else {
        panic!("generated text expected")
    };
    assert_eq!(output.text, "Translated response");
    private(&output);
    let Output::MediaModels(models) = execute(&fixture.client, Command::ListMediaModels).await
    else {
        panic!("media models expected")
    };
    assert!(
        models.iter().any(
            |model| model.provider == provider.id && model.kinds.contains(&media::Kind::Vision)
        )
    );
    execute(
        &fixture.client,
        Command::SaveMediaSettings(media::Settings {
            revision: 0,
            bindings: [(
                media::Kind::Vision,
                Some(media::Binding {
                    provider: provider.id,
                    model: "translated".into(),
                }),
            )]
            .into(),
        }),
    )
    .await;
    let Output::Snapshot(snapshot) = execute(&fixture.client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let worktree = snapshot.worktrees[0].id;
    let Output::Session(session) = execute(
        &fixture.client,
        Command::CreateSession {
            project: snapshot.worktrees[0].project,
            worktree: Some(worktree),
            config: Some(SessionConfig {
                assistant: None,
                resource: None,
                provider: provider.id,
                credential: provider.credential.clone(),
                model: "translated".into(),
                effort: Effort::Default,
                mode: WorkMode::Code,
                permission: Permission::Full,
                options: Default::default(),
            }),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    let Output::QueuedTurn(turn) = execute(
        &fixture.client,
        Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Hold the parent".into(),
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    let requests = server.requests.lock().unwrap().len();
    execute(&fixture.client, Command::StartQueuedTurn { turn: turn.id }).await;
    server.wait_requests(requests + 1).await;
    let image = fixture.directory.path().join("source/image.png");
    // A complete one-pixel PNG keeps the real file/format owner in the contract.
    std::fs::write(image,base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jS1sAAAAASUVORK5CYII=").unwrap()).unwrap();
    context.worktree = Some(worktree);
    context.session = Some(session.id);
    context.turn = Some(turn.id);
    let Output::Media(output) = fixture
        .client
        .execute(
            fixture
                .client
                .prepare(Command::UseMedia {
                    turn: turn.id,
                    session: session.id,
                    worktree,
                    action: media::Action::Inspect {
                        prompt: "Describe this image".into(),
                        source: media::Source::Path("image.png".into()),
                    },
                })
                .with_plugin(context),
        )
        .await
        .unwrap()
    else {
        panic!("vision result expected")
    };
    assert_eq!(output["text"], "Translated response");
    private(&output);
    execute(&fixture.client, Command::StopTurn { turn: turn.id }).await;
    provider
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preserves_public_endpoint_metadata() {
    use support::{execute, info};
    const HOST: &str = "127.0.0.1";
    for remote in [false, true] {
        let mode = Arc::new(AtomicUsize::new(0));
        let selected = mode.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            match request.path.as_str() {
                "/token" => Reply::Json(json!({"data":{"access":FRESH,"refresh":ROTATED,
                    "expires_in":3600,"account":ACCOUNT,"domain":HOST}})),
                "/catalog" => {
                    assert_eq!(request.headers["authorization"], format!("Bearer {FRESH}"));
                    assert_eq!(request.headers["x-domain"], HOST);
                    assert_eq!(request.headers["x-account"], ACCOUNT);
                    let echoed = match selected.load(Ordering::SeqCst) {
                        0 => format!("https://{HOST}/models"),
                        1 => FRESH.into(),
                        2 => ROTATED.into(),
                        _ => ACCOUNT.into(),
                    };
                    Reply::Json(json!({"documentation":echoed,"catalog":[{"id":"translated","context":64000,"output":4096}]}))
                }
                _ => panic!("unexpected endpoint metadata route"),
            }
        }).await;
        let fixture = Fixture::empty(remote, &server).await;
        let mut provider = configured_channel(&fixture, true, json!({"kind":"token_exchange","token":{"url":"https://auth.example.test/token","method":"POST"}}), Some(&server.endpoint)).await;
        let installed = info(
            execute(
                &fixture.client,
                Command::ReadPlugin {
                    name: "example".into(),
                },
            )
            .await,
        );
        let path = fixture.directory.path().join("source/package/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["version"] = json!("1.0.1");
        let declaration = &mut manifest["extensions"]["dev.sailry.platform"]["model_auth"][0];
        declaration["token"]["extra"]["domain"] = json!("/domain");
        declaration["inject"]
            .as_array_mut()
            .unwrap()
            .push(json!({"header":"X-Domain","template":"{extra.domain}"}));
        std::fs::write(path, manifest.to_string()).unwrap();
        let Output::Snapshot(snapshot) = execute(&fixture.client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let updated = info(
            execute(
                &fixture.client,
                support::install(snapshot.worktrees[0].id, installed.summary.revision),
            )
            .await,
        );
        provider.consent.as_mut().unwrap().package = updated.summary.reference();
        provider.consent.as_mut().unwrap().origin = None;
        let Output::Provider(provider) = execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut updates = fixture.client.subscribe_login(attempt.id).await.unwrap();
        let update = terminal(&mut updates).await;
        private(&update);
        assert_eq!(update.state, login::State::Connected);
        assert_eq!(update.model_error, None);
        let provider = saved(&fixture.client, provider.id).await;
        assert_eq!(provider.models[0].id, "translated");
        for value in [1, 2, 3] {
            mode.store(value, Ordering::SeqCst);
            let error = fixture
                .client
                .execute(fixture.client.prepare(models(&provider)))
                .await
                .unwrap_err();
            private(&error);
            assert_eq!(error.code, ErrorCode::PermissionDenied);
        }
        mode.store(0, Ordering::SeqCst);
        let Output::DiscoveredModels(catalog) = execute(&fixture.client, models(&provider)).await
        else {
            panic!("catalog expected")
        };
        assert_eq!(catalog.models[0].id, "translated");
        assert_eq!(
            saved(&fixture.client, provider.id).await.credential,
            provider.credential
        );
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request.path == "/catalog")
                    .count(),
                5
            );
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request.path == "/token")
                    .count(),
                1
            );
        }
        fixture.close().await;
    }
}
