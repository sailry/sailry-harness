use super::discovery::server::{Reply, Server};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sailry_node_runtime::Node;
use sailry_protocol::conversation::ModelApi;
use serde_json::json;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn resumes_authorization() {
    for authentication in ["chat_gpt", "copilot"] {
        let directory = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let token = format!("e30.{}.fixture", URL_SAFE_NO_PAD.encode(json!({"exp":4102444800_u64,"https://api.openai.com/auth":{"chatgpt_account_id":"ffi-account"}}).to_string()));
        let access = token.clone();
        let server = runtime.block_on(Server::start_with_request(ModelApi::Anthropic, move |request| {
            match request.path.as_str() {
                path if path.starts_with("/models") => {
                    assert_eq!(request.method, "GET");
                    assert_eq!(request.headers["authorization"], format!("Bearer {}", if authentication == "chat_gpt" { &access } else { "ffi-copilot-access" }));
                    if authentication == "chat_gpt" {
                        assert!(path.starts_with("/models?client_version="));
                        assert_eq!(request.headers["chatgpt-account-id"], "ffi-account");
                        Reply::Json(json!({"models":[{"slug":"ffi-login","context_window":8192,"max_output_tokens":512,"input_modalities":["text","image"],"supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],"default_reasoning_level":"high"}]}))
                    } else {
                        assert_eq!(request.headers["x-initiator"], "user");
                        Reply::Json(json!({"data":[{"id":"ffi-login","supported_endpoints":["/responses"],"capabilities":{"type":"chat","limits":{"max_context_window_tokens":8192,"max_output_tokens":512},"supports":{"vision":true,"tool_calls":true,"reasoning_effort":["low","high"]}}}]}))
                    }
                }
                "/api/accounts/deviceauth/usercode" => Reply::Json(json!({"device_auth_id":"ffi-device-secret","user_code":"FFI-1234","interval":1})),
                "/api/accounts/deviceauth/token" => Reply::Json(json!({"authorization_code":"ffi-code-secret","code_verifier":"ffi-verifier-secret"})),
                "/oauth/token" => Reply::Json(json!({"access_token":access,"refresh_token":"ffi-refresh-secret"})),
                "/login/device/code" => Reply::Json(json!({"device_code":"ffi-device-secret","user_code":"FFI-1234","verification_uri":"https://github.com/login/device","interval":1,"expires_in":900})),
                "/login/oauth/access_token" => Reply::Json(json!({"access_token":"ffi-github-secret"})),
                "/copilot_internal/v2/token" => Reply::Json(json!({"token":"ffi-copilot-access","expires_at":4102444800_u64})),
                "/responses" => {
                    assert_eq!(request.headers["authorization"], format!("Bearer {}", if authentication == "chat_gpt" { &access } else { "ffi-copilot-access" }));
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
            3
        );
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
