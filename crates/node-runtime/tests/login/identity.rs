//! Synthetic signed identity fixture for the public-client ChatGPT flow.
use super::*;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::Url;
use serde_json::Value;
use sha2::Digest;
use std::collections::BTreeMap;

pub(super) const REDIRECT: &str = "http://127.0.0.1:54321/auth/callback";
pub(super) const CLIENT: &str = "oaiapp_fixture";
pub(super) const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

// RFC 8032 section 7.1 test vector 1. These published bytes are not credentials.
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const PUBLIC: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

pub(super) fn issuer(request: &discovery_support::Request) -> String {
    format!("http://{}", request.headers["host"])
}

pub(super) fn keys() -> Value {
    json!({"keys":[{"kty":"OKP", "crv":"Ed25519", "alg":"EdDSA", "kid":"fixture-key",
        "use":"sig", "key_ops":["verify"], "x":URL_SAFE_NO_PAD.encode(PUBLIC)}]})
}

pub(super) fn signed(claims: &Value) -> String {
    let mut key = vec![
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ];
    key.extend_from_slice(&SEED);
    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = Some("fixture-key".into());
    jsonwebtoken::encode(&header, claims, &EncodingKey::from_ed_der(&key)).unwrap()
}

pub(super) fn form(request: &discovery_support::Request) -> BTreeMap<String, String> {
    Url::parse(&format!("http://fixture/?{}", request.body))
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

pub(super) fn tokens(request: &discovery_support::Request, expires: u64, refresh: &str) -> Value {
    let form = form(request);
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.headers["content-type"],
        "application/x-www-form-urlencoded"
    );
    assert_eq!(form["client_id"], CLIENT);
    assert_eq!(form["resource"], "https://api.openai.com/v1");
    assert!(!form.contains_key("client_secret"));
    let mut value = json!({"access_token":access_token(),"refresh_token":refresh,
        "token_type":"Bearer", "expires_in":expires,"scope":SCOPES,"earliest_refresh_at":1790032532});
    match form["grant_type"].as_str() {
        "authorization_code" => {
            assert_eq!(form["redirect_uri"], REDIRECT);
            let code: Value =
                serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&form["code"]).unwrap()).unwrap();
            let challenge =
                URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(form["code_verifier"].as_bytes()));
            assert_eq!(code["challenge"], challenge);
            value["id_token"] = json!(signed(&json!({"iss":issuer(request),"aud":CLIENT,
                "sub":"isolated-chatgpt-subject", "email":"user@example.com",
                "nonce":code["nonce"], "iat":future_expiry()-3600,"exp":future_expiry()})));
        }
        "refresh_token" => {
            assert!(!form.contains_key("scope"));
            assert!(form["refresh_token"].starts_with("isolated-"));
        }
        _ => panic!("unexpected token grant"),
    }
    value
}

pub(super) fn callback(authorization: &str) -> Secret {
    let url = Url::parse(authorization).unwrap();
    let fields: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(url.path(), "/api/accounts/authorize");
    assert_eq!(fields["resource"], "https://api.openai.com/v1");
    assert_eq!(fields["scope"], SCOPES);
    assert_eq!(fields["redirect_uri"], REDIRECT);
    assert_eq!(fields["code_challenge_method"], "S256");
    assert!(!fields.contains_key("id_token_hint"));
    if fields["client_id"] == "dynamic_agent_client" {
        assert_eq!(fields["agent_name_hint"], "Sailry");
    } else {
        assert_eq!(fields["client_id"], CLIENT);
        assert!(!fields.contains_key("agent_name_hint"));
    }
    let code = URL_SAFE_NO_PAD
        .encode(json!({"nonce":fields["nonce"],"challenge":fields["code_challenge"]}).to_string());
    let mut callback = Url::parse(REDIRECT).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("state", &fields["state"])
        .append_pair("code", &code)
        .append_pair("client_id", CLIENT);
    Secret::new(callback.into())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retains_registration_and_host_after_expired_code() {
    for remote in [false, true] {
        let exchanges = std::sync::atomic::AtomicUsize::new(0);
        let profile = Arc::new(std::sync::Mutex::new(None::<std::path::PathBuf>));
        let stored = profile.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/api/accounts/oauth/token" {
                let db = rusqlite::Connection::open_with_flags(
                    stored.lock().unwrap().as_ref().unwrap(),
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                ).unwrap();
                let registration: String = db.query_row("SELECT client_id FROM oauth_registrations", [], |row| row.get(0)).unwrap();
                assert_eq!(registration, CLIENT, "registration must be durable before exchange");
                if exchanges.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    Reply::Raw("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"error\":\"invalid_grant\"}".into())
                } else { standard(request) }
            } else { standard(request) }
        }).await;
        let fixture = Fixture::empty(remote, &server).await;
        *profile.lock().unwrap() = Some(fixture.node.profile().join("storage/node.sqlite3"));
        let provider = provider(&fixture.client, Authentication::ChatGpt).await;
        let (_, first) = start(&fixture.client, &provider).await;
        let first_url = browser(&fixture.client, &first).await;
        let first_fields: BTreeMap<_, _> = Url::parse(&first_url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        assert_eq!(first_fields["client_id"], "dynamic_agent_client");
        let returned = callback(&first_url);
        let private_url = returned.expose().to_owned();
        let callback_request = complete(&fixture.client, &first, returned).await;
        let mut stream = fixture.client.subscribe_login(first.id).await.unwrap();
        assert!(matches!(
            terminal(&mut stream).await.state,
            login::State::Failed(_)
        ));
        let Output::Credentials(credentials) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListCredentials))
            .await
            .unwrap()
        else {
            panic!("credentials expected")
        };
        assert!(credentials.is_empty());
        let db = rusqlite::Connection::open_with_flags(
            fixture.node.profile().join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut query = db
            .prepare("SELECT CAST(body AS TEXT),coalesce(result,'') FROM requests")
            .unwrap();
        for row in query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
        {
            let (body, result) = row.unwrap();
            assert!(!body.contains(&private_url));
            assert!(!result.contains(&private_url));
            no_secrets(&body);
            no_secrets(&result);
        }
        drop(query);
        drop(db);
        assert_eq!(
            fixture.client.execute(callback_request).await.unwrap(),
            Output::ProviderLogin(first)
        );
        let fixture = execution::renewals::restart(fixture, &server, remote).await;
        let (_, second) = start(&fixture.client, &provider).await;
        let second_url = browser(&fixture.client, &second).await;
        let second_fields: BTreeMap<_, _> = Url::parse(&second_url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        assert_eq!(second_fields["client_id"], CLIENT);
        assert_eq!(
            second_fields["ext_agent_host_id"],
            first_fields["ext_agent_host_id"]
        );
        assert!(!second_fields.contains_key("agent_name_hint"));
        assert_ne!(second_fields["state"], first_fields["state"]);
        complete(&fixture.client, &second, callback(&second_url)).await;
        let mut stream = fixture.client.subscribe_login(second.id).await.unwrap();
        assert_eq!(terminal(&mut stream).await.state, login::State::Connected);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requires_durable_registration_before_exchange() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, |_| {
            panic!("exchange must not start without a durable registration")
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = provider(&fixture.client, Authentication::ChatGpt).await;
        let db = rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
            .unwrap();
        db.execute_batch("CREATE TRIGGER reject_registration BEFORE INSERT ON oauth_registrations BEGIN SELECT RAISE(ABORT,'isolated registration failure'); END;").unwrap();
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        let login::State::Failed(error) = terminal(&mut stream).await.state else {
            panic!("registration storage failure expected")
        };
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(server.requests.lock().unwrap().is_empty());
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ListCredentials))
                .await
                .unwrap(),
            Output::Credentials(vec![])
        );
        drop(db);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retains_sign_in_without_plan_permission() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/api/accounts/oauth/token" {
                let mut tokens = tokens(request, 3600, REFRESH);
                tokens["scope"] = json!("openid email profile offline_access");
                Reply::Json(tokens)
            } else {
                assert!(
                    !request.path.starts_with("/models"),
                    "plan-disabled sign-in must not invoke models"
                );
                standard(request)
            }
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = provider(&fixture.client, Authentication::ChatGpt).await;
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        let update = terminal(&mut stream).await;
        assert_eq!(update.state, login::State::Connected);
        assert_eq!(update.model_error.unwrap().code, ErrorCode::NotConfigured);
        let Output::Providers(providers) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        assert!(providers[0].credential.is_some());
        assert!(providers[0].models.is_empty());
        no_secrets(&providers);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnects_after_revocation() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, standard).await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider =
            execution::signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
        let reference = provider.credential.as_ref().unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::RevokeCredential {
                id: reference.id,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        let (_, attempt) = start(&fixture.client, &provider).await;
        let authorization = browser(&fixture.client, &attempt).await;
        let fields: BTreeMap<_, _> = Url::parse(&authorization)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        assert_eq!(fields["client_id"], CLIENT);
        complete(&fixture.client, &attempt, callback(&authorization)).await;
        let mut updates = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert_eq!(terminal(&mut updates).await.state, login::State::Connected);
        let Output::Credentials(credentials) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListCredentials))
            .await
            .unwrap()
        else {
            panic!("credentials expected")
        };
        assert_eq!(credentials.len(), 1);
        assert_eq!(credentials[0].id, reference.id);
        assert_eq!(credentials[0].revision, 3);
        assert!(!credentials[0].revoked);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_identity_mismatch_without_replacing_account() {
    for remote in [false, true] {
        let count = std::sync::atomic::AtomicUsize::new(0);
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/api/accounts/oauth/token" {
                let mut value = tokens(request, 3600, REFRESH);
                if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0 {
                    let fields = form(request);
                    let code: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&fields["code"]).unwrap()).unwrap();
                    value["id_token"] = json!(signed(&json!({"iss":issuer(request),"aud":CLIENT,
                        "sub":"another-account", "nonce":code["nonce"],"iat":future_expiry()-3600,"exp":future_expiry()})));
                }
                Reply::Json(value)
            } else { standard(request) }
        }).await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider =
            execution::signed_in(&fixture, Authentication::ChatGpt, ModelApi::Responses).await;
        let before = fixture
            .client
            .execute(fixture.client.prepare(Command::ListCredentials))
            .await
            .unwrap();
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert!(matches!(
            terminal(&mut stream).await.state,
            login::State::Failed(_)
        ));
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ListCredentials))
                .await
                .unwrap(),
            before
        );
        let Output::Providers(providers) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        assert_eq!(providers[0], provider);
        fixture.close().await;
    }
}
