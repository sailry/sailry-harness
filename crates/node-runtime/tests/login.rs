#![cfg(feature = "test-support")]
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sailry_client::Client;
use sailry_link::{Link, NetworkScope, Subscription};
use sailry_node_runtime::Node;
use sailry_protocol::{
    conversation::{ModelApi, Provider, discovery, login},
    *,
};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[allow(dead_code)]
mod discovery_support;
use discovery_support::{Reply, Server};
#[path = "login/catalog.rs"]
mod catalog;
#[path = "login/execution.rs"]
mod execution;
#[path = "login/lifecycle.rs"]
mod lifecycle;
#[path = "login/responses.rs"]
mod responses;

const REFRESH: &str = "isolated-refresh-secret";
const GITHUB: &str = "isolated-github-secret";
const ACCESS: &str = "isolated-copilot-secret";
const DEVICE: &str = "isolated-device-secret";

struct Fixture {
    directory: tempfile::TempDir,
    node: Node,
    controller: Link,
    client: Arc<Client>,
}

impl Fixture {
    async fn new(remote: bool, server: &Server) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_with_authorization(directory.path().join("node"), &server.endpoint)
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Arc::new(Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        }));
        // Authorization fixtures must never contact shipped external MCP services.
        for name in ["context7", "github"] {
            let Output::Plugin(package) = client
                .execute(client.prepare(Command::ReadPlugin { name: name.into() }))
                .await
                .unwrap()
            else {
                panic!("shipped package expected")
            };
            client
                .execute(client.prepare(Command::SetPluginEnabled {
                    name: name.into(),
                    expected_revision: package.summary.revision,
                    enabled: false,
                }))
                .await
                .unwrap();
        }
        Self {
            directory,
            node,
            controller,
            client,
        }
    }
    async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

fn future_expiry() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600
}
fn access_token() -> String {
    format!("e30.{}.fixture", URL_SAFE_NO_PAD.encode(json!({"exp": future_expiry(), "https://api.openai.com/auth": {"chatgpt_account_id":"fixture-account"}}).to_string()))
}

fn standard(request: &discovery_support::Request) -> Reply {
    match request.path.as_str() {
        "/api/accounts/deviceauth/usercode" => {
            assert_eq!(request.method, "POST");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&request.body).unwrap()["client_id"],
                "app_EMoamEEZ73f0CkXaXp7hrann"
            );
            Reply::Json(json!({"device_auth_id": DEVICE,"user_code":"ABCD-1234","interval":"1"}))
        }
        "/api/accounts/deviceauth/token" => {
            let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
            assert_eq!(body["device_auth_id"], DEVICE);
            assert_eq!(body["user_code"], "ABCD-1234");
            Reply::Json(
                json!({"authorization_code":"isolated-code-secret","code_verifier":"isolated-verifier-secret","code_challenge":"fixture"}),
            )
        }
        "/oauth/token" => {
            assert_eq!(
                request.headers["content-type"],
                "application/x-www-form-urlencoded"
            );
            assert!(request.body.contains("grant_type=authorization_code"));
            assert!(
                request
                    .body
                    .contains("code_verifier=isolated-verifier-secret")
            );
            Reply::Json(json!({"access_token":access_token(),"refresh_token":REFRESH}))
        }
        "/login/device/code" => {
            assert_eq!(request.headers["accept"], "application/json");
            assert!(request.body.contains("client_id=Iv1.b507a08c87ecfe98"));
            Reply::Json(
                json!({"device_code":DEVICE,"user_code":"ABCD-1234","verification_uri":"https://github.com/login/device","interval":1,"expires_in":900}),
            )
        }
        "/login/oauth/access_token" => {
            assert!(request.body.contains("device_code=isolated-device-secret"));
            assert!(
                request
                    .body
                    .contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code")
            );
            Reply::Json(json!({"access_token":GITHUB}))
        }
        "/copilot_internal/v2/token" => {
            assert_eq!(request.method, "GET");
            assert_eq!(request.headers["authorization"], format!("token {GITHUB}"));
            Reply::Json(
                json!({"token":ACCESS,"expires_at":future_expiry(),"endpoints":{"api":"https://api.individual.githubcopilot.com"}}),
            )
        }
        _ => panic!("unexpected authorization endpoint"),
    }
}

async fn provider(client: &Client, authentication: Authentication) -> Provider {
    let provider = Provider {
        options: None,
        id: ProviderId::new(),
        revision: 0,
        name: "Authorization fixture".into(),
        api: ModelApi::Responses,
        authentication,
        endpoint: if authentication == Authentication::ChatGpt {
            "https://chatgpt.com/backend-api/codex"
        } else {
            "https://api.githubcopilot.com"
        }
        .into(),
        enabled: true,
        models: vec![],
        default_model: "fixture".into(),
        credential: None,
    };
    let Output::Provider(provider) = client
        .execute(client.prepare(Command::PutProvider {
            provider,
            expected_revision: 0,
        }))
        .await
        .unwrap()
    else {
        panic!("provider expected")
    };
    provider
}

async fn begin(client: &Client, provider: &Provider) -> (Request, login::Attempt) {
    let request = client.prepare(Command::BeginProviderLogin {
        provider: provider.id,
        expected_revision: provider.revision,
    });
    let admission = client.dispatch(request.clone()).await.unwrap();
    assert!(admission.receipt.durable);
    let Output::ProviderLogin(attempt) = admission.completion.await.unwrap().unwrap() else {
        panic!("attempt expected")
    };
    assert_eq!(attempt.id, request.id);
    (request, attempt)
}

async fn next(stream: &mut Box<dyn Subscription>) -> login::Update {
    let Update::ProviderLogin(update) =
        tokio::time::timeout(Duration::from_secs(10), stream.next())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("login update expected")
    };
    update
}

async fn terminal(stream: &mut Box<dyn Subscription>) -> login::Update {
    loop {
        let update = next(stream).await;
        if !update.state.active() {
            return update;
        }
    }
}

async fn subscription_error(result: Result<Box<dyn Subscription>, Fault>) -> Fault {
    match result {
        Err(error) => error,
        Ok(mut stream) => tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap_err(),
    }
}

fn no_secrets(value: &impl serde::Serialize) {
    let body = serde_json::to_string(value).unwrap();
    for secret in [
        REFRESH,
        GITHUB,
        ACCESS,
        DEVICE,
        "isolated-code-secret",
        "isolated-verifier-secret",
        "fixture-account",
    ] {
        assert!(!body.contains(secret));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persists_and_recovers() {
    for remote in [false, true] {
        for authentication in [Authentication::ChatGpt, Authentication::Copilot] {
            let server = Server::start_with_request(ModelApi::Anthropic, standard).await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = provider(&fixture.client, authentication).await;
            let (request, attempt) = begin(&fixture.client, &provider).await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            let mut prompt_seen = false;
            loop {
                let update = next(&mut stream).await;
                no_secrets(&update);
                if let login::State::Pending {
                    user_code,
                    verification_url,
                    ..
                } = &update.state
                {
                    assert_eq!(user_code, "ABCD-1234");
                    assert!(verification_url.starts_with("https://"));
                    prompt_seen = true;
                }
                if !update.state.active() {
                    assert_eq!(update.state, login::State::Connected);
                    break;
                }
            }
            assert!(prompt_seen);
            let Output::Providers(providers) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListProviders))
                .await
                .unwrap()
            else {
                panic!("providers expected")
            };
            let saved = providers[0].clone();
            assert_eq!(saved.revision, 2);
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ReadProviderKey {
                        provider: saved.id,
                        expected_revision: saved.revision,
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
            let reference = saved.credential.clone().unwrap();
            let credentials = fixture
                .client
                .execute(fixture.client.prepare(Command::ListCredentials))
                .await
                .unwrap();
            no_secrets(&credentials);
            let Output::Credentials(credentials) = credentials else {
                panic!("credentials expected")
            };
            assert_eq!(credentials[0].authentication, authentication);
            assert_eq!(credentials[0].expires_at_ms, None);
            assert_eq!(
                fixture
                    .node
                    .resolve_credential(reference.clone(), saved.id)
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
            let failure = fixture
                .client
                .execute(fixture.client.prepare(Command::DiscoverModels(Box::new(
                    discovery::Source::Draft(discovery::Draft {
                        provider: saved.id,
                        api: ModelApi::Responses,
                        endpoint: server.endpoint.clone(),
                        credential: Some(reference.clone()),
                        secret: None,
                    }),
                ))))
                .await
                .unwrap_err();
            assert_eq!(failure.code, ErrorCode::PermissionDenied);
            let result = fixture.client.execute(request.clone()).await.unwrap();
            assert_eq!(result, Output::ProviderLogin(attempt.clone()));
            no_secrets(&result);
            no_secrets(
                &fixture
                    .client
                    .execute(fixture.client.prepare(Command::Snapshot))
                    .await
                    .unwrap(),
            );
            assert_eq!(server.requests.lock().unwrap().len(), 3);
            let path = fixture.directory.path().join("node");
            drop(stream);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
            let node = Node::start_with_authorization(&path, &server.endpoint)
                .await
                .unwrap();
            let controller = Link::controller(
                fixture.directory.path().join("controller"),
                NetworkScope::default(),
            )
            .await
            .unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            assert_eq!(
                client.execute(request).await.unwrap(),
                Output::ProviderLogin(attempt.clone())
            );
            assert_eq!(
                subscription_error(client.subscribe_login(attempt.id).await)
                    .await
                    .code,
                ErrorCode::NotFound
            );
            assert_eq!(server.requests.lock().unwrap().len(), 3);
            assert_eq!(
                client
                    .execute(client.prepare(Command::ListProviders))
                    .await
                    .unwrap(),
                Output::Providers(vec![saved])
            );
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}
