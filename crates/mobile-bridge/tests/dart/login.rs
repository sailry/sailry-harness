use super::discovery::server::{Reply, Request, Server};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{
    Command, Output,
    conversation::{ModelApi, oauth::DEFAULT_CATALOG_VERSION},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "login/identity.rs"]
mod identity;

const CLIENT: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ACCESS: &str = "ffi-codex-access";

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn resumes_authorization() {
    for authentication in ["chat_gpt_oauth", "copilot"] {
        let directory = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(Server::start_with_request(ModelApi::Anthropic, move |request| {
            match request.path.as_str() {
                path if path.starts_with("/models") => {
                    assert_eq!(request.method, "GET");
                    assert_eq!(request.headers["authorization"], format!("Bearer {}", if authentication == "chat_gpt_oauth" { ACCESS } else { "ffi-copilot-access" }));
                    if authentication == "chat_gpt_oauth" {
                        assert_eq!(path, format!("/models?client_version={DEFAULT_CATALOG_VERSION}"));
                        assert_eq!(request.headers["chatgpt-account-id"], "ffi-account");
                        Reply::Json(json!({"models":[{"slug":"ffi-login","visibility":"list","display_name":"FFI login","context_window":8192,"max_output_tokens":512,"input_modalities":["text","image"],"supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],"default_reasoning_level":"high"}]}))
                    } else {
                        assert_eq!(request.headers["x-initiator"], "user");
                        Reply::Json(json!({"data":[{"id":"ffi-login","supported_endpoints":["/responses"],"capabilities":{"type":"chat","limits":{"max_context_window_tokens":8192,"max_output_tokens":512},"supports":{"vision":true,"tool_calls":true,"reasoning_effort":["low","high"]}}}]}))
                    }
                }
                "/oauth/token" => Reply::Json(tokens(request)),
                "/.well-known/openid-configuration" => {
                    let issuer = format!("http://{}", request.headers["host"]);
                    Reply::Json(json!({"issuer":issuer,"jwks_uri":format!("{issuer}/.well-known/jwks.json")}))
                }
                "/.well-known/jwks.json" => Reply::Json(identity::keys()),
                "/login/device/code" => Reply::Json(json!({"device_code":"ffi-device-secret","user_code":"FFI-1234","verification_uri":"https://github.com/login/device","interval":1,"expires_in":900})),
                "/login/oauth/access_token" => Reply::Json(json!({"access_token":"ffi-github-secret"})),
                "/copilot_internal/v2/token" => Reply::Json(json!({"token":"ffi-copilot-access","expires_at":4102444800_u64})),
                "/responses" => {
                    assert_eq!(request.headers["authorization"], format!("Bearer {}", if authentication == "chat_gpt_oauth" { ACCESS } else { "ffi-copilot-access" }));
                    let delta = json!({"type":"response.output_text.delta","sequence_number":0,"item_id":"ffi-message","output_index":0,"content_index":0,"delta":"FFI authorized response","logprobs":[]});
                    let completed = json!({"type":"response.completed","sequence_number":1,"response":{"id":"ffi-response","object":"response","created_at":1,"status":"completed","model":"ffi-login","output":[{"type":"message","id":"ffi-message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"FFI authorized response","annotations":[]}]}]}});
                    Reply::Raw(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {delta}\n\ndata: {completed}\n\ndata: [DONE]\n\n"))
                }
                _ => panic!("unexpected FFI authorization request"),
            }
        }));
        let node = runtime
            .block_on(Node::start_with_authorization(
                directory.path().join("node"),
                &server.endpoint,
            ))
            .unwrap();
        runtime.block_on(async {
            let client = Client::new(node.local());
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
        });
        let invitation = node.link().invite().unwrap();
        let child = super::command("login.dart", directory.path(), invitation.ticket())
            .env("SAILRY_AUTHENTICATION", authentication)
            .spawn()
            .unwrap();
        let status = super::wait(child);
        runtime.block_on(node.shutdown()).unwrap();
        assert!(status.success());
        let requests = server.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| matches!(
                    request.path.as_str(),
                    "/api/accounts/deviceauth/usercode" | "/login/device/code"
                ))
                .count(),
            if authentication == "chat_gpt_oauth" {
                0
            } else {
                3
            }
        );
        if authentication == "chat_gpt_oauth" {
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request.path == "/oauth/token")
                    .count(),
                2
            );
        }
        // Two completed logins refresh models, followed by discovery and validation.
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.path.starts_with("/models"))
                .count(),
            4
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.path == "/responses")
                .count(),
            1
        );
    }
}

fn tokens(request: &Request) -> Value {
    let form: BTreeMap<_, _> = url::form_urlencoded::parse(request.body.as_bytes())
        .into_owned()
        .collect();
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.headers["content-type"],
        "application/x-www-form-urlencoded"
    );
    assert_eq!(form["grant_type"], "authorization_code");
    assert_eq!(form["client_id"], CLIENT);
    assert_eq!(form["redirect_uri"], "http://localhost:1455/auth/callback");
    assert_eq!(form["code"], "ffi-code-secret");
    assert_eq!(form["code_verifier"].len(), 43);
    for field in ["resource", "scope", "client_secret"] {
        assert!(!form.contains_key(field));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    json!({"access_token":ACCESS,"refresh_token":"ffi-refresh-secret","token_type":"Bearer","expires_in":3600,
        "id_token":identity::signed(&json!({"iss":format!("http://{}",request.headers["host"]),"aud":CLIENT,
            "sub":"ffi-codex-subject","email":"codex@example.com","iat":now,"exp":now+3600,
            "https://api.openai.com/auth":{"chatgpt_account_id":"ffi-account"}}))})
}
