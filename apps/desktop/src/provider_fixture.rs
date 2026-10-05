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

pub(crate) async fn server(mode: Arc<AtomicU8>) -> Server {
    Server::start_with_request(ModelApi::Anthropic, move |request| {
        if mode.load(Ordering::SeqCst) == 2 {
            return Reply::Raw("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
        }
        match request.path.as_str() {
            path if path.starts_with("/models") && mode.load(Ordering::SeqCst) == 5 => Reply::Raw("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()),
            path if path.starts_with("/models") && mode.load(Ordering::SeqCst) == 3 => Reply::Hold,
            path if path.starts_with("/models") && mode.load(Ordering::SeqCst) == 6 => Reply::Json(if request.headers.contains_key("chatgpt-account-id") { json!({"models":[]}) } else { json!({"data":[]}) }),
            path if path.starts_with("/models") => Reply::Json(models(request.headers.contains_key("chatgpt-account-id"))),
            "/api/accounts/deviceauth/usercode" => Reply::Json(json!({"device_auth_id":"desktop-device-secret","user_code":"SAIL-1234","interval":1})),
            "/login/device/code" => Reply::Json(json!({"device_code":"desktop-device-secret","user_code":"SAIL-1234","verification_uri":"https://github.com/login/device","interval":1,"expires_in":900})),
            "/login/oauth/access_token" if mode.load(Ordering::SeqCst) == 4 => Reply::Json(json!({"error":"expired_token"})),
            "/api/accounts/deviceauth/token" | "/login/oauth/access_token" if mode.load(Ordering::SeqCst) == 0 => Reply::Hold,
            "/api/accounts/deviceauth/token" => Reply::Json(json!({"authorization_code":"desktop-code-secret","code_verifier":"desktop-verifier-secret"})),
            "/oauth/token" => {
                // Synthetic JWT: expiry 4102444800 and account desktop-fixture.
                let token = "e30.eyJleHAiOjQxMDI0NDQ4MDAsImh0dHBzOi8vYXBpLm9wZW5haS5jb20vYXV0aCI6eyJjaGF0Z3B0X2FjY291bnRfaWQiOiJkZXNrdG9wLWZpeHR1cmUifX0K.fixture";
                Reply::Json(json!({"access_token":token,"refresh_token":"desktop-refresh-secret"}))
            }
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
            {"slug":"fixture", "context_window":16384,"max_output_tokens":1024,
                "input_modalities":["text","image"],"supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],"default_reasoning_level":"high"},
            {"slug":"unknown", "context_window":32768}
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
            "https://chatgpt.com/backend-api/codex"
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
        .execute(client.prepare(Command::BeginProviderLogin {
            provider: provider.id,
            expected_revision: provider.revision,
        }))
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
