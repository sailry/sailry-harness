use super::*;
use sailry_protocol::conversation::{consent, presets};
use sha2::Digest;
use std::sync::atomic::{AtomicUsize, Ordering};

#[allow(dead_code)]
#[path = "../plugins/support.rs"]
pub(super) mod support;

#[path = "package/adapter.rs"]
mod adapter;
#[path = "package/headers.rs"]
mod headers;
#[path = "package/partners.rs"]
mod partners;
#[path = "package/recovery.rs"]
mod recovery;
#[path = "package/renewal.rs"]
mod renewal;
#[path = "package/resources.rs"]
mod resources;

const INITIAL: &str = "isolated-package-access";
const FRESH: &str = "isolated-package-fresh";
const RENEWAL: &str = "isolated-package-refresh";
const ROTATED: &str = "isolated-package-rotated";
const ACCOUNT: &str = "isolated-package-account";
const CHALLENGE: &str = "isolated-package-state+/=";
const CODE: &str = "isolated-package-code";
const DEVICE_STATE: &str = "isolated-package-device";

fn private<T: serde::Serialize>(value: &T) {
    let value = serde_json::to_string(value).unwrap();
    for secret in [
        INITIAL,
        FRESH,
        RENEWAL,
        ROTATED,
        ACCOUNT,
        CHALLENGE,
        CODE,
        DEVICE_STATE,
    ] {
        assert!(!value.contains(secret));
    }
}

fn tokens(fresh: bool) -> Reply {
    Reply::Json(json!({"data":{"access":if fresh {FRESH} else {INITIAL},
        "refresh":if fresh {ROTATED} else {RENEWAL},
        "expires_in":if fresh {3600} else {30},"account":ACCOUNT}}))
}

async fn configured(fixture: &Fixture, approved: bool) -> Provider {
    configured_flow(fixture, approved, json!({"kind":"token_exchange","token":{"url":"https://auth.example.test/token","method":"POST"}})).await
}

async fn configured_flow(fixture: &Fixture, approved: bool, flow: serde_json::Value) -> Provider {
    configured_channel(fixture, approved, flow, None, None).await
}

async fn configured_channel(
    fixture: &Fixture,
    approved: bool,
    flow: serde_json::Value,
    endpoint: Option<&str>,
    native: Option<(ModelApi, serde_json::Value)>,
) -> Provider {
    use support::{execute, info, install, package};
    let source = fixture.directory.path().join("source");
    let root = source.join("package");
    package(&root, "1.0.0");
    let (api, inject) = native.unwrap_or_else(|| {
        (
            ModelApi::ChatCompletions,
            json!([
                {"header":"Authorization","template":"Bearer {access}"},
                {"header":"X-Account","template":"{extra.account}"}
            ]),
        )
    });
    let protocol = match api {
        ModelApi::ChatCompletions => "openai-chat-completions",
        ModelApi::Responses => "openai-responses",
        ModelApi::Anthropic => "anthropic",
        _ => panic!("native fixture protocol expected"),
    };
    let mut preset = json!({"id":"example","name":"Example channel","routes":[{"protocol":protocol,"endpoint":"https://model.example.test"}],"auth":{"kind":"plugin","data":{"flow":"example"}}});
    if let Some(endpoint) = endpoint {
        preset["routes"][0] = json!({"protocol":"plugin","endpoint":endpoint});
        preset["adapter"] = json!("example");
    }
    let declaration = json!({"id":"example","display":{"label":"Example sign-in"},
        "flow":flow,
        "client":{"client_id":"example-public-client"},
        "token":{"access":"/access","refresh":"/refresh","expires_in":"/expires_in","extra":{"account":"/account"}},
        "inject":inject,
        "refresh":{"request":{"url":"https://auth.example.test/refresh","method":"POST","headers":{"X-Refresh-Token":"{refresh}"}},"token":{"access":"/access","refresh":"/refresh","expires_in":"/expires_in","extra":{"account":"/account"}}},
        "exchange":{"script":{"entry":"dev.sailry.platform/auth.js","resources":["dev.sailry.platform/auth.js"]},"handler":"exchange"}});
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("plugin.json")).unwrap()).unwrap();
    manifest["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1","actions":["models.authenticate","credentials.inspect"],"model_providers":[preset],"model_auth":[declaration]}});
    if endpoint.is_some() {
        manifest["extensions"]["dev.sailry.platform"]["actions"]
            .as_array_mut()
            .unwrap()
            .extend([
                json!("models.provide"),
                json!("models.generate"),
                json!("media.inspect"),
            ]);
        manifest["extensions"]["dev.sailry.platform"]["model_adapters"] = json!([{"id":"example","script":{"entry":"dev.sailry.platform/adapter.js"},"origins":[],"framing":"json","discovery":true,"capabilities":{"vision":true}}]);
        std::fs::write(root.join("dev.sailry.platform/adapter.js"), r#"
export function prepare_discovery(input) {
  if(Object.keys(input).join(',')!=='endpoint') throw Error('Unexpected discovery input');
  return {method:'GET',url:input.endpoint+'/catalog',headers:{'X-Catalog':'declared'},body:null,framing:'json'};
}
export function decode_discovery(input) {
  const value=JSON.parse(input.body);
  return {endpoint:input.endpoint,models:value.catalog};
}
export function prepare(input) {
  return {method:'POST',url:input.endpoint+'/generate',headers:{'content-type':'application/json'},body:JSON.stringify(input),framing:'json'};
}
export function decode(input) {
  if(!input.done) return {state:input.state,deltas:[]};
  return {state:null,deltas:input.events.flatMap(event=>[{kind:'text',text:JSON.parse(event.data).content},{kind:'finish',reason:'stop'}])};
}
"#).unwrap();
    }
    std::fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/auth.js"),
        "export function exchange(value) { return value.data; }",
    )
    .unwrap();
    execute(
        &fixture.client,
        Command::RegisterProject {
            name: "Authentication fixture".into(),
            path: source.to_str().unwrap().into(),
        },
    )
    .await;
    let Output::Snapshot(snapshot) = execute(&fixture.client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let installed = info(execute(&fixture.client, install(snapshot.worktrees[0].id, 0)).await);
    assert!(installed.issues.is_empty(), "{:?}", installed.issues);
    let Output::ChannelPresets(listing) = execute(
        &fixture.client,
        Command::ListChannelPresets {
            refresh_partners: false,
        },
    )
    .await
    else {
        panic!("channel listing expected")
    };
    let entry = listing
        .entries
        .iter()
        .find(|entry| matches!(&entry.source,presets::Source::Package{name} if name=="example"))
        .unwrap();
    assert_eq!(
        entry.requested,
        consent::Scopes {
            messages: endpoint.is_some(),
            credentials: true
        }
    );
    assert!(entry.origin.is_some());
    let provider:Provider=serde_json::from_value(json!({"id":ProviderId::new(),"revision":0,"name":"Example","api":api,"authentication":"plugin","endpoint":"https://model.example.test","enabled":true,"models":[],"default_model":"fixture","credential":null,
        "preset":{"source":entry.source,"id":"example","route":entry.preset.routes[0].key()},
        "consent":{"package":installed.summary.reference(),"scopes":{"messages":false,"credentials":approved},"origin":null}})).unwrap();
    let mut provider = provider;
    if let Some(endpoint) = endpoint {
        provider.api = ModelApi::Plugin;
        provider.endpoint = endpoint.to_owned();
    }
    let Output::Provider(provider) = execute(
        &fixture.client,
        Command::PutProvider {
            provider,
            expected_revision: 0,
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    assert!(provider.consent.as_ref().unwrap().origin.is_none());
    provider
}

async fn saved(client: &Client, id: ProviderId) -> Provider {
    let Output::Providers(providers) = client
        .execute(client.prepare(Command::ListProviders))
        .await
        .unwrap()
    else {
        panic!("providers expected")
    };
    private(&providers);
    providers
        .into_iter()
        .find(|provider| provider.id == id)
        .unwrap()
}

fn models(provider: &Provider) -> Command {
    Command::DiscoverModels(Box::new(discovery::Source::Saved {
        provider: provider.id,
        expected_revision: provider.revision,
        oauth: None,
    }))
}

async fn credentials(client: &Client) -> Vec<Credential> {
    let Output::Credentials(values) = client
        .execute(client.prepare(Command::ListCredentials))
        .await
        .unwrap()
    else {
        panic!("credentials expected")
    };
    private(&values);
    values
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exchanges_declared_pkce_callbacks() {
    const REDIRECT: &str = "http://127.0.0.1:54321/auth/callback";
    let challenge = Arc::new(std::sync::Mutex::new(None::<String>));
    let expected = challenge.clone();
    let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
        match request.path.as_str() {
            "/token" => {
                assert_eq!(request.method, "POST");
                assert_eq!(
                    request.headers["content-type"],
                    "application/x-www-form-urlencoded"
                );
                let form = identity::form(request);
                assert_eq!(form["grant_type"], "authorization_code");
                assert_eq!(form["client_id"], "example-public-client");
                assert_eq!(form["code"], CODE);
                assert_eq!(form["redirect_uri"], REDIRECT);
                assert_eq!(
                    Some(
                        URL_SAFE_NO_PAD
                            .encode(sha2::Sha256::digest(form["code_verifier"].as_bytes()))
                    ),
                    *expected.lock().unwrap()
                );
                tokens(true)
            }
            "/models" => {
                assert_eq!(request.headers["authorization"], format!("Bearer {FRESH}"));
                assert_eq!(request.headers["x-account"], ACCOUNT);
                Reply::Json(catalog::models(Authentication::Plugin))
            }
            _ => panic!("unexpected fixture request"),
        }
    })
    .await;
    let fixture = Fixture::empty(false, &server).await;
    let provider = configured_flow(
        &fixture,
        true,
        json!({"kind":"pkce",
        "authorize":"https://auth.example.test/authorize",
        "token":{"url":"https://auth.example.test/token","method":"POST"},
        "redirect":"loopback","redirect_uri":null}),
    )
    .await;
    let request = fixture.client.prepare(Command::BeginProviderLogin {
        provider: provider.id,
        expected_revision: provider.revision,
        redirect: Some(REDIRECT.into()),
    });
    let admission = fixture.client.dispatch(request.clone()).await.unwrap();
    assert!(admission.receipt.durable);
    let Output::ProviderLogin(attempt) = admission.completion.await.unwrap().unwrap() else {
        panic!("login attempt expected")
    };
    assert_eq!(attempt.id, request.id);
    let authorization = browser(&fixture.client, &attempt).await;
    let url = reqwest::Url::parse(&authorization).unwrap();
    assert_eq!(
        url.origin().ascii_serialization(),
        "https://auth.example.test"
    );
    assert_eq!(url.path(), "/authorize");
    let query = url
        .query_pairs()
        .into_owned()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(query["client_id"], "example-public-client");
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["redirect_uri"], REDIRECT);
    assert!(!query["state"].is_empty());
    *challenge.lock().unwrap() = Some(query["code_challenge"].clone());
    assert!(server.requests.lock().unwrap().is_empty());
    let mut callback = reqwest::Url::parse(REDIRECT).unwrap();
    callback
        .query_pairs_mut()
        .extend_pairs([("code", CODE), ("state", query["state"].as_str())]);
    complete(&fixture.client, &attempt, Secret::new(callback.into())).await;
    let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
    let update = terminal(&mut stream).await;
    private(&update);
    assert_eq!(update.state, login::State::Connected);
    assert_eq!(update.model_error, None);
    assert_eq!(credentials(&fixture.client).await.len(), 1);
    let provider = saved(&fixture.client, provider.id).await;
    assert!(provider.credential.is_some());
    assert!(matches!(
        fixture
            .client
            .execute(fixture.client.prepare(models(&provider)))
            .await
            .unwrap(),
        Output::DiscoveredModels(_)
    ));
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn polls_declared_device_codes() {
    let polls = Arc::new(AtomicUsize::new(0));
    let polled = polls.clone();
    let release = Arc::new(tokio::sync::Notify::new());
    let verified = release.clone();
    let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
        match request.path.as_str() {
            "/device" => {
                assert_eq!(request.method, "POST");
                let form = identity::form(request);
                assert_eq!(form["client_id"], "example-public-client");
                Reply::Json(json!({"verification_uri":"https://auth.example.test/device","user_code":"ABCD-EFGH","device_code":DEVICE_STATE,"expires_in":600,"interval":1}))
            }
            "/token" => {
                assert_eq!(request.method, "POST");
                let form = identity::form(request);
                assert_eq!(form["client_id"], "example-public-client");
                assert_eq!(form["grant_type"], "urn:ietf:params:oauth:grant-type:device_code");
                assert_eq!(form["device_code"], DEVICE_STATE);
                match polled.fetch_add(1, Ordering::SeqCst) {
                    0 => Reply::Json(json!({"error":"authorization_pending"})),
                    1 => Reply::Delayed(verified.clone(), json!({"data":{"access":FRESH,"refresh":ROTATED,"expires_in":3600,"account":ACCOUNT}})),
                    _ => panic!("completed device code was polled again"),
                }
            }
            "/models" => {
                assert_eq!(request.headers["authorization"], format!("Bearer {FRESH}"));
                assert_eq!(request.headers["x-account"], ACCOUNT);
                Reply::Json(catalog::models(Authentication::Plugin))
            }
            _ => panic!("unexpected fixture request"),
        }
    }).await;
    let fixture = Fixture::empty(true, &server).await;
    let provider = configured_flow(
        &fixture,
        true,
        json!({"kind":"device_code",
        "device":{"url":"https://auth.example.test/device","method":"POST"},
        "token":{"url":"https://auth.example.test/token","method":"POST"}}),
    )
    .await;
    let (request, attempt) = begin(&fixture.client, &provider).await;
    let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
    let mut pending = false;
    loop {
        let update = next(&mut stream).await;
        private(&update);
        match update.state {
            login::State::Starting => {}
            login::State::Pending {
                verification_url,
                user_code,
                ..
            } => {
                assert_eq!(verification_url, "https://auth.example.test/device");
                assert_eq!(user_code, "ABCD-EFGH");
                pending = true;
                release.notify_one();
            }
            login::State::Connected => {
                assert_eq!(update.model_error, None);
                break;
            }
            state => panic!("unexpected device state: {state:?}"),
        }
    }
    assert!(pending);
    assert_eq!(polls.load(Ordering::SeqCst), 2);
    assert_eq!(
        fixture.client.execute(request).await.unwrap(),
        Output::ProviderLogin(attempt)
    );
    assert_eq!(credentials(&fixture.client).await.len(), 1);
    let provider = saved(&fixture.client, provider.id).await;
    assert!(provider.credential.is_some());
    assert!(matches!(
        fixture
            .client
            .execute(fixture.client.prepare(models(&provider)))
            .await
            .unwrap(),
        Output::DiscoveredModels(_)
    ));
    assert_eq!(polls.load(Ordering::SeqCst), 2);
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn completes_private_polling_challenges() {
    for remote in [false, true] {
        let starts = Arc::new(AtomicUsize::new(0));
        let polls = Arc::new(AtomicUsize::new(0));
        let started = starts.clone();
        let polled = polls.clone();
        let release = Arc::new(tokio::sync::Notify::new());
        let verified = release.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            match request.path.as_str() {
                "/challenge" => {
                    assert_eq!(request.method, "POST");
                    assert_eq!(serde_json::from_str::<serde_json::Value>(&request.body).unwrap(), json!({}));
                    assert_eq!(started.fetch_add(1, Ordering::SeqCst), 0);
                    Reply::Json(json!({"data":{"state":CHALLENGE,"url":"https://auth.example.test/verify"}}))
                }
                path if path.starts_with("/poll?") => {
                    assert_eq!(request.method, "GET");
                    assert!(request.body.is_empty());
                    let url = reqwest::Url::parse(&format!("http://fixture{path}")).unwrap();
                    let query = url.query_pairs().into_owned().collect::<std::collections::BTreeMap<_,_>>();
                    assert_eq!(query.len(), 1);
                    assert_eq!(query["state"], CHALLENGE);
                    match polled.fetch_add(1, Ordering::SeqCst) {
                        0 => Reply::Json(json!({"code":202})),
                        1 => Reply::Delayed(verified.clone(), json!({"code":0,"data":{"access":FRESH,"refresh":ROTATED,"expires_in":3600,"account":ACCOUNT}})),
                        _ => panic!("completed challenge was polled again"),
                    }
                }
                "/models" => {
                    assert_eq!(request.headers["authorization"], format!("Bearer {FRESH}"));
                    assert_eq!(request.headers["x-account"], ACCOUNT);
                    Reply::Json(catalog::models(Authentication::Plugin))
                }
                _ => panic!("unexpected fixture request"),
            }
        }).await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = configured_flow(&fixture, true, json!({
            "kind":"polling",
            "start":{"url":"https://auth.example.test/challenge","method":"POST","body":{"kind":"json","value":{}}},
            "state":"/data/state","verification_url":"/data/url",
            "token":{"url":"https://auth.example.test/poll","method":"GET","query":{"state":"{state}"}},
            "success":{"pointer":"/code","values":[0]},"interval_ms":1
        })).await;
        let (request, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        let mut browser = false;
        loop {
            let update = next(&mut stream).await;
            private(&update);
            match update.state {
                login::State::Starting => {}
                login::State::Browser {
                    authorization_url,
                    expires_at_ms,
                } => {
                    assert_eq!(authorization_url, "https://auth.example.test/verify");
                    assert!(expires_at_ms > future_expiry().saturating_sub(3600) * 1000);
                    browser = true;
                    release.notify_one();
                }
                login::State::Connected => {
                    assert_eq!(update.model_error, None);
                    break;
                }
                state => panic!("unexpected polling state: {state:?}"),
            }
        }
        assert!(browser);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert_eq!(polls.load(Ordering::SeqCst), 2);
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::ProviderLogin(attempt)
        );
        let provider = saved(&fixture.client, provider.id).await;
        assert!(provider.credential.is_some());
        assert_eq!(credentials(&fixture.client).await.len(), 1);
        assert!(matches!(
            fixture
                .client
                .execute(fixture.client.prepare(models(&provider)))
                .await
                .unwrap(),
            Output::DiscoveredModels(_)
        ));
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert_eq!(polls.load(Ordering::SeqCst), 2);
        let Output::Snapshot(snapshot) = fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        private(&snapshot);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signs_in_and_refreshes_bound_grants() {
    for remote in [false, true] {
        let exchanges = Arc::new(AtomicUsize::new(0));
        let rotations = Arc::new(AtomicUsize::new(0));
        let initial = exchanges.clone();
        let fresh = rotations.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| match request
            .path
            .as_str()
        {
            "/token" => {
                initial.fetch_add(1, Ordering::SeqCst);
                tokens(false)
            }
            "/refresh" => {
                fresh.fetch_add(1, Ordering::SeqCst);
                assert_eq!(request.headers["x-refresh-token"], RENEWAL);
                assert!(request.body.is_empty());
                tokens(true)
            }
            "/models" => {
                assert_eq!(request.headers["x-account"], ACCOUNT);
                assert!(
                    [format!("Bearer {INITIAL}"), format!("Bearer {FRESH}")]
                        .contains(&request.headers["authorization"])
                );
                Reply::Json(catalog::models(Authentication::Plugin))
            }
            _ => panic!("unexpected fixture request"),
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = configured(&fixture, true).await;
        let (request, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        let update = terminal(&mut stream).await;
        private(&update);
        assert_eq!(update.state, login::State::Connected);
        assert_eq!(update.model_error, None);
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::ProviderLogin(attempt)
        );
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);
        let provider = saved(&fixture.client, provider.id).await;
        assert!(provider.credential.is_some());
        assert!(provider.binding.is_none());
        assert!(matches!(
            fixture
                .client
                .execute(fixture.client.prepare(models(&provider)))
                .await
                .unwrap(),
            Output::DiscoveredModels(_)
        ));
        assert_eq!(rotations.load(Ordering::SeqCst), 1);
        assert_eq!(credentials(&fixture.client).await.len(), 1);
        drop(stream);
        let fixture = execution::renewals::restart(fixture, &server, remote).await;
        assert!(matches!(
            fixture
                .client
                .execute(fixture.client.prepare(models(&provider)))
                .await
                .unwrap(),
            Output::DiscoveredModels(_)
        ));
        assert_eq!(rotations.load(Ordering::SeqCst), 1);
        let Output::Snapshot(snapshot) = fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        private(&snapshot);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trusts_installed_private_transformations() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, |request| {
            match request.path.as_str() {
                "/token" => tokens(true),
                "/models" => Reply::Json(catalog::models(Authentication::Plugin)),
                _ => panic!("unexpected trusted authentication route"),
            }
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = configured(&fixture, false).await;
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut updates = fixture.client.subscribe_login(attempt.id).await.unwrap();
        let update = terminal(&mut updates).await;
        private(&update);
        assert_eq!(update.state, login::State::Connected);
        assert_eq!(update.model_error, None);
        assert_eq!(credentials(&fixture.client).await.len(), 1);
        assert!(
            !saved(&fixture.client, provider.id)
                .await
                .consent
                .unwrap()
                .scopes
                .credentials
        );
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancels_or_rejects_changed_attempts() {
    for remote in [false, true] {
        for cancelled in [false, true] {
            let release = Arc::new(tokio::sync::Notify::new());
            let held = release.clone();
            let server=Server::start_with_request(ModelApi::Anthropic,move|request|match request.path.as_str(){
            "/token"=>Reply::Delayed(held.clone(),json!({"data":{"access":INITIAL,"refresh":RENEWAL,"expires_in":3600,"account":ACCOUNT}})),
            "/models"=>Reply::Json(catalog::models(Authentication::Plugin)),
            _=>panic!("unexpected fixture request"),
        }).await;
            let fixture = Fixture::empty(remote, &server).await;
            let mut provider = configured(&fixture, true).await;
            let (_, attempt) = begin(&fixture.client, &provider).await;
            server.wait_requests(1).await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            if cancelled {
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::CancelProviderLogin {
                        attempt: attempt.id,
                    }))
                    .await
                    .unwrap();
            } else {
                provider.name = "Changed channel".into();
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::PutProvider {
                        expected_revision: provider.revision,
                        provider,
                    }))
                    .await
                    .unwrap();
            }
            release.notify_one();
            let update = terminal(&mut stream).await;
            private(&update);
            if cancelled {
                assert_eq!(update.state, login::State::Cancelled);
            } else {
                let login::State::Failed(error) = update.state else {
                    panic!("changed attempt must fail")
                };
                assert_eq!(error.code, ErrorCode::RevisionConflict);
            }
            assert!(credentials(&fixture.client).await.is_empty());
            fixture.close().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn never_replays_an_uncertain_rotation() {
    for remote in [false, true] {
        let rotations = Arc::new(AtomicUsize::new(0));
        let count = rotations.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| match request
            .path
            .as_str()
        {
            "/token" => tokens(false),
            "/refresh" => {
                count.fetch_add(1, Ordering::SeqCst);
                Reply::Raw(String::new())
            }
            "/models" => Reply::Json(catalog::models(Authentication::Plugin)),
            _ => panic!("unexpected fixture request"),
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = configured(&fixture, true).await;
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert_eq!(terminal(&mut stream).await.state, login::State::Connected);
        let provider = saved(&fixture.client, provider.id).await;
        for _ in 0..2 {
            let error = fixture
                .client
                .execute(fixture.client.prepare(models(&provider)))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::OutcomeUnknown);
            private(&error);
        }
        assert_eq!(rotations.load(Ordering::SeqCst), 1);
        assert_eq!(credentials(&fixture.client).await.len(), 1);
        drop(stream);
        let fixture = execution::renewals::restart(fixture, &server, remote).await;
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(models(&provider)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(rotations.load(Ordering::SeqCst), 1);
        fixture.close().await;
    }
}
