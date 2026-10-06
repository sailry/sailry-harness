use sailry_protocol::{
    Authentication, ProviderId,
    conversation::{ModelApi, Provider},
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use crate::discovery_fixture::Reply;
pub(crate) use crate::discovery_fixture::Server;
mod identity;
pub(crate) use identity::callback;

pub(crate) async fn server(mode: Arc<AtomicU8>) -> Server {
    start(mode, None).await
}

pub(crate) async fn server_with_gate(
    mode: Arc<AtomicU8>,
    gate: Arc<tokio::sync::Notify>,
) -> Server {
    start(mode, Some(gate)).await
}

async fn start(mode: Arc<AtomicU8>, gate: Option<Arc<tokio::sync::Notify>>) -> Server {
    Server::start_with_request(ModelApi::Anthropic, move |request| {
        if mode.load(Ordering::SeqCst) == 2 {
            return Reply::Raw("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
        }
        match request.path.as_str() {
            "/.well-known/openid-configuration" => Reply::Json(json!({"issuer":identity::issuer(request),"jwks_uri":format!("{}/.well-known/jwks.json",identity::issuer(request))})),
            "/.well-known/jwks.json" => Reply::Json(identity::keys()),
            path if path.starts_with("/models") && mode.load(Ordering::SeqCst) == 5 => Reply::Raw("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()),
            path if path.starts_with("/models") && mode.load(Ordering::SeqCst) == 3 => match &gate {
                Some(gate) => Reply::Delayed(gate.clone(), models(!request.headers.contains_key("editor-version"))),
                None => Reply::Hold,
            },
            path if path.starts_with("/models") && mode.load(Ordering::SeqCst) == 6 => Reply::Json(if request.headers.contains_key("editor-version") { json!({"data":[]}) } else { json!({"models":[]}) }),
            path if path.starts_with("/models") => Reply::Json(models(!request.headers.contains_key("editor-version"))),
            "/login/device/code" => Reply::Json(json!({"device_code":"desktop-device-secret","user_code":"SAIL-1234","verification_uri":"https://github.com/login/device","interval":1,"expires_in":900})),
            "/login/oauth/access_token" if mode.load(Ordering::SeqCst) == 4 => Reply::Json(json!({"error":"expired_token"})),
            "/api/accounts/oauth/token" | "/login/oauth/access_token" if mode.load(Ordering::SeqCst) == 0 => Reply::Hold,
            "/api/accounts/oauth/token" => Reply::Json(identity::tokens(request)),
            "/login/oauth/access_token" => Reply::Json(json!({"access_token":"desktop-github-secret"})),
            "/copilot_internal/v2/token" => Reply::Json(json!({"token":"desktop-copilot-secret","expires_at":4102444800_u64})),
            "/responses" => response(),
            _ => panic!("unexpected desktop authorization request: {}", request.path),
        }
    }).await
}

fn models(chatgpt: bool) -> serde_json::Value {
    if chatgpt {
        json!({"models":[
            {"slug":"fixture", "display_name":"Fixture model", "visibility":"list", "context_window":16384,"max_output_tokens":1024,
                "input_modalities":["text","image"],"supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],"default_reasoning_level":"high"},
            {"slug":"unknown", "display_name":"Unknown model", "visibility":"list", "context_window":32768}
        ]})
    } else {
        json!({"data":[
            {"id":"fixture", "supported_endpoints":["/responses","/chat/completions"],
                "capabilities":{"type":"chat", "limits":{"max_context_window_tokens":16384,"max_output_tokens":1024},
                    "supports":{"vision":true,"tool_calls":true,"reasoning_effort":["low","high"]}}},
            {"id":"unknown", "supported_endpoints":["/responses","/chat/completions"],
                "capabilities":{"type":"chat","limits":{"max_context_window_tokens":32768}}}
        ]})
    }
}

pub(crate) fn provider(authentication: Authentication, api: ModelApi) -> Provider {
    Provider {
        options: None,
        oauth: None,
        id: ProviderId::new(),
        revision: 0,
        name: if authentication == Authentication::ChatGpt {
            "ChatGPT"
        } else {
            "GitHub Copilot"
        }
        .into(),
        api,
        authentication,
        endpoint: if authentication == Authentication::ChatGpt {
            "https://api.openai.com/v1"
        } else {
            "https://api.githubcopilot.com"
        }
        .into(),
        enabled: true,
        credential: None,
        default_model: "fixture".into(),
        models: vec![],
    }
}

fn response() -> Reply {
    let output = json!([{"type":"message","id":"message-fixture","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Authorized response","annotations":[]}]}]);
    let completed = json!({"type":"response.completed","sequence_number":1,"response":{"id":"response-fixture","object":"response","created_at":1,"status":"completed","model":"fixture","output":output}});
    let delta = json!({"type":"response.output_text.delta","sequence_number":0,"item_id":"message-fixture","output_index":0,"content_index":0,"delta":"Authorized response","logprobs":[]});
    Reply::Raw(format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {delta}\n\ndata: {completed}\n\ndata: [DONE]\n\n"
    ))
}

pub(crate) async fn connect(client: &sailry_client::Client, provider: Provider) -> Provider {
    use sailry_protocol::{Command, Output, Update, conversation::login};
    let Output::Provider(provider) = client
        .execute(client.prepare(Command::SaveProvider {
            provider,
            expected_revision: 0,
            secret: None,
        }))
        .await
        .unwrap()
    else {
        panic!("provider expected")
    };
    let Output::ProviderLogin(attempt) = client
        .execute(
            client.prepare(Command::BeginProviderLogin {
                provider: provider.id,
                expected_revision: provider.revision,
                redirect: (provider.authentication == Authentication::ChatGpt)
                    .then(|| "http://127.0.0.1:54321/auth/callback".into()),
            }),
        )
        .await
        .unwrap()
    else {
        panic!("login expected")
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut stream = client.subscribe_login(attempt.id).await.unwrap();
        loop {
            let Update::ProviderLogin(update) = stream.next().await.unwrap() else {
                panic!("login update expected")
            };
            if let login::State::Browser {
                authorization_url, ..
            } = &update.state
            {
                client
                    .execute(client.prepare(Command::CompleteProviderLogin {
                        attempt: attempt.id,
                        callback: callback(authorization_url),
                    }))
                    .await
                    .unwrap();
            }
            if !update.state.active() {
                assert_eq!(update.state, login::State::Connected);
                break;
            }
        }
    })
    .await
    .unwrap();
    let Output::Providers(providers) = client
        .execute(client.prepare(Command::ListProviders))
        .await
        .unwrap()
    else {
        panic!("providers expected")
    };
    providers
        .into_iter()
        .find(|value| value.id == provider.id)
        .unwrap()
}
