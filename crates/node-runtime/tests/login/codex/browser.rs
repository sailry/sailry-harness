use super::*;
use sailry_protocol::conversation::presets;
use sha2::{Digest, Sha256};
use std::sync::Mutex;

const REDIRECT: &str = "http://localhost:1455/auth/callback";

async fn channel(fixture: &Fixture) -> Provider {
    let mut provider = provider(&fixture.client, Authentication::ChatGptOAuth).await;
    provider.preset = Some(presets::PresetRef {
        source: presets::Source::Builtin,
        id: "chat-gpt-oauth".into(),
        route: None,
    });
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
    provider
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exchanges_and_refreshes_on_execution_node() {
    for remote in [false, true] {
        for manual in [false, true] {
            let challenge = Arc::new(Mutex::new(String::new()));
            let expected = challenge.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path == "/oauth/token" {
                    let form = identity::form(request);
                    assert_eq!(form["client_id"], CLIENT);
                    let refreshing = form["grant_type"] == "refresh_token";
                    if refreshing {
                        assert_eq!(form["refresh_token"], REFRESH);
                        assert!(!form.contains_key("redirect_uri"));
                    } else {
                        assert_eq!(form["grant_type"], "authorization_code");
                        assert_eq!(form["redirect_uri"], REDIRECT);
                        assert_eq!(form["code"], CODE);
                        assert_eq!(URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes())), *expected.lock().unwrap());
                    }
                    return Reply::Json(json!({
                        "access_token":if refreshing { REFRESHED } else { ACCESS },
                        "refresh_token":if refreshing { ROTATED } else { REFRESH },
                        "token_type":"Bearer", "expires_in":if refreshing { 3600 } else { 30 },
                        "id_token":identity::signed(&json!({"sub":"isolated-codex-subject", "email":"codex@example.com",
                            "https://api.openai.com/auth":{"chatgpt_account_id":ACCOUNT}}))
                    }));
                }
                if request.path.starts_with("/models") {
                    let access = if request.headers["authorization"] == format!("Bearer {ACCESS}") { ACCESS } else { REFRESHED };
                    return server::catalog(request, &Options::defaults(Authentication::ChatGptOAuth).unwrap(), access);
                }
                server::standard(request, 3600)
            }).await;
            let fixture = Fixture::empty(remote, &server).await;
            let provider = channel(&fixture).await;
            let request = fixture.client.prepare(Command::BeginProviderLogin {
                provider: provider.id,
                expected_revision: provider.revision,
                redirect: (!manual).then(|| REDIRECT.into()),
            });
            let Output::ProviderLogin(attempt) =
                fixture.client.execute(request.clone()).await.unwrap()
            else {
                panic!("attempt expected")
            };
            let url = super::super::browser(&fixture.client, &attempt).await;
            let parsed = url::Url::parse(&url).unwrap();
            let query = parsed
                .query_pairs()
                .into_owned()
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(parsed.path(), "/oauth/authorize");
            assert_eq!(query["redirect_uri"], REDIRECT);
            assert_eq!(query["code_challenge_method"], "S256");
            assert!(!query.contains_key("code_verifier"));
            *challenge.lock().unwrap() = query["code_challenge"].clone();
            complete(
                &fixture.client,
                &attempt,
                Secret::new(format!("{REDIRECT}?code={CODE}&state={}", query["state"])),
            )
            .await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            let update = terminal(&mut stream).await;
            assert_eq!(update.state, login::State::Connected);
            assert_eq!(update.model_error, None);
            let saved = saved(&fixture.client, provider.id).await;
            assert!(saved.credential.is_some());
            no_secrets(&saved);
            assert_eq!(saved.models.len(), 2);
            let Output::DiscoveredModels(discovered) = fixture
                .client
                .execute(fixture.client.prepare(Command::DiscoverModels(Box::new(
                    discovery::Source::Saved {
                        provider: saved.id,
                        expected_revision: saved.revision,
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
                fixture.client.execute(request).await.unwrap(),
                Output::ProviderLogin(attempt)
            );
            assert_eq!(server.requests.lock().unwrap().len(), before);
            {
                let requests = server.requests.lock().unwrap();
                assert_eq!(
                    requests
                        .iter()
                        .filter(|request| request.path == "/oauth/token")
                        .count(),
                    2
                );
                assert!(
                    !requests
                        .iter()
                        .any(|request| request.path.contains("deviceauth"))
                );
            }
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_wrong_redirect_and_callback_state() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, |_| {
            panic!("invalid callback must not exchange tokens")
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = channel(&fixture).await;
        let invalid = fixture
            .client
            .execute(fixture.client.prepare(Command::BeginProviderLogin {
                provider: provider.id,
                expected_revision: provider.revision,
                redirect: Some("http://localhost:1456/auth/callback".into()),
            }))
            .await
            .unwrap_err();
        assert_eq!(invalid.code, ErrorCode::InvalidRequest);
        let (_, attempt) = start(&fixture.client, &provider).await;
        super::super::browser(&fixture.client, &attempt).await;
        complete(
            &fixture.client,
            &attempt,
            Secret::new(format!("{REDIRECT}?code={CODE}&state=incorrect")),
        )
        .await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert!(matches!(
            terminal(&mut stream).await.state,
            login::State::Failed(_)
        ));
        assert!(
            saved(&fixture.client, provider.id)
                .await
                .credential
                .is_none()
        );
        assert!(server.requests.lock().unwrap().is_empty());
        fixture.close().await;
    }
}
