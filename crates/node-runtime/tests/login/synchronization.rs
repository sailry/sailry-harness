use super::*;

const ACCOUNTS: [(Authentication, ModelApi); 3] = [
    (Authentication::ChatGpt, ModelApi::Responses),
    (Authentication::Copilot, ModelApi::Responses),
    (Authentication::Copilot, ModelApi::ChatCompletions),
];

async fn create(client: &Client, authentication: Authentication, api: ModelApi) -> Provider {
    let mut provider = provider(client, authentication).await;
    if provider.api != api {
        provider.api = api;
        let Output::Provider(saved) = client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        provider = saved;
    }
    provider
}

async fn saved(client: &Client, id: ProviderId) -> Provider {
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    no_secrets(&snapshot);
    snapshot
        .providers
        .into_iter()
        .find(|provider| provider.id == id)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn seeds_and_reconnects() {
    for remote in [false, true] {
        for (authentication, api) in ACCOUNTS {
            let server = Server::start_with_request(ModelApi::Anthropic, standard).await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = create(&fixture.client, authentication, api).await;
            assert!(provider.models.is_empty());
            let (request, attempt) = begin(&fixture.client, &provider).await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            let update = terminal(&mut stream).await;
            assert_eq!(update.state, login::State::Connected);
            assert_eq!(update.model_error, None);
            no_secrets(&update);
            let mut provider = saved(&fixture.client, provider.id).await;
            assert_eq!(
                provider
                    .models
                    .iter()
                    .map(|model| model.id.as_str())
                    .collect::<Vec<_>>(),
                ["fixture", "unknown"]
            );
            assert_eq!(provider.default_model, "fixture");
            let native = &provider.models[0];
            assert_eq!(
                (native.context, native.output),
                if authentication == Authentication::ChatGpt {
                    (4096, 4096)
                } else {
                    (16384, 1024)
                }
            );
            assert!(native.vision && native.reasoning);
            assert_eq!(native.efforts, [Effort::Low, Effort::High]);
            assert_eq!(
                native.default_effort,
                if authentication == Authentication::ChatGpt {
                    Effort::High
                } else {
                    Effort::Low
                }
            );
            let count = server.requests.lock().unwrap().len();
            assert_eq!(
                fixture.client.execute(request).await.unwrap(),
                Output::ProviderLogin(attempt)
            );
            assert_eq!(server.requests.lock().unwrap().len(), count);
            provider.models.remove(0);
            provider.models[0].context = 8192;
            provider.models[0].output = 512;
            provider.models[0].reasoning = true;
            provider.models[0].efforts = vec![Effort::High];
            provider.models[0].default_effort = Effort::High;
            provider.models[0].custom_efforts = true;
            provider.default_model = "unknown".into();
            provider.enabled = false;
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
            let (_, attempt) = begin(&fixture.client, &provider).await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            let update = terminal(&mut stream).await;
            assert_eq!(update.state, login::State::Connected);
            assert_eq!(update.model_error, None);
            let reconnected = saved(&fixture.client, provider.id).await;
            assert_eq!(reconnected.models, provider.models);
            assert_eq!(reconnected.default_model, provider.default_model);
            assert_eq!(reconnected.enabled, provider.enabled);
            assert_eq!(reconnected.credential, provider.credential);
            assert_eq!(reconnected.revision, provider.revision + 1);
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retains_grants_on_catalog_failure() {
    for remote in [false, true] {
        for (authentication, api) in ACCOUNTS {
            for response in ["denied", "empty", "malformed"] {
                let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                    if !request.path.starts_with("/models") { return standard(request); }
                    match response {
                        "denied" => Reply::Raw("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()),
                        "empty" => Reply::Json(if authentication == Authentication::ChatGpt { json!({"models":[]}) } else { json!({"data":[]}) }),
                        _ => Reply::Json(json!({"invalid":"isolated malformed catalog"})),
                    }
                }).await;
                let fixture = Fixture::new(remote, &server).await;
                let provider = create(&fixture.client, authentication, api).await;
                let (_, attempt) = begin(&fixture.client, &provider).await;
                let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
                let update = terminal(&mut stream).await;
                assert_eq!(update.state, login::State::Connected);
                assert_eq!(
                    update.model_error.as_ref().unwrap().code,
                    if response == "denied" {
                        ErrorCode::NotConfigured
                    } else {
                        ErrorCode::Unavailable
                    }
                );
                no_secrets(&update);
                let saved = saved(&fixture.client, provider.id).await;
                assert!(saved.credential.is_some());
                assert!(saved.models.is_empty());
                assert_eq!(saved.default_model, provider.default_model);
                assert_eq!(saved.revision, provider.revision + 1);
                let Output::Credentials(credentials) = fixture
                    .client
                    .execute(fixture.client.prepare(Command::ListCredentials))
                    .await
                    .unwrap()
                else {
                    panic!("credentials expected")
                };
                assert_eq!(credentials.len(), 1);
                assert!(!credentials[0].revoked);
                fixture.close().await;
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn completes_from_the_cached_reference() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, standard).await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = create(
            &fixture.client,
            Authentication::ChatGpt,
            ModelApi::Responses,
        )
        .await;
        let db = rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
            .unwrap();
        let reference = json!({"id":"unknown","limit":{"context":65536,"output":2048},"tool_call":true,"reasoning":true,"modalities":{"input":["text","image"],"output":["text"]},"reasoning_options":[{"type":"effort","values":["low","high"]}]});
        db.execute(
            "INSERT INTO model_catalog(provider,id,body) VALUES('openai','unknown',?1)",
            [serde_json::to_vec(&reference).unwrap()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO model_catalog_status(id,body) VALUES(1,?1)",
            [
                serde_json::to_vec(&sailry_protocol::conversation::catalog::Status {
                    revision: 1,
                    models: 1,
                    providers: 1,
                    ..Default::default()
                })
                .unwrap(),
            ],
        )
        .unwrap();
        drop(db);
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        let update = terminal(&mut stream).await;
        assert_eq!(update.state, login::State::Connected);
        assert_eq!(update.model_error, None);
        let provider = saved(&fixture.client, provider.id).await;
        let unknown = &provider.models[1];
        assert_eq!((unknown.context, unknown.output), (65536, 2048));
        assert!(unknown.vision && unknown.tools && unknown.reasoning);
        assert_eq!(unknown.efforts, [Effort::Low, Effort::High]);
        assert_eq!(unknown.default_effort, Effort::Low);
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        fixture.close().await;
    }
}
