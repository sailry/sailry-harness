use super::*;
use serde_json::Value;

pub(crate) fn standard(request: &discovery_support::Request, expires: u64) -> Reply {
    match request.path.as_str() {
        "/v1/responses"
            if request.method == "GET"
                && request
                    .headers
                    .get("upgrade")
                    .is_some_and(|value| value.eq_ignore_ascii_case("websocket")) =>
        {
            Reply::Raw(
                "HTTP/1.1 426 Upgrade Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .into(),
            )
        }
        "/api/accounts/deviceauth/usercode" => {
            assert_eq!(request.method, "POST");
            assert_eq!(
                serde_json::from_str::<Value>(&request.body).unwrap(),
                json!({"client_id":CLIENT})
            );
            Reply::Json(
                json!({"device_auth_id":DEVICE,"user_code":USER_CODE,"interval":1,"expires_in":900}),
            )
        }
        "/api/accounts/deviceauth/token" => {
            assert_eq!(request.method, "POST");
            assert_eq!(
                serde_json::from_str::<Value>(&request.body).unwrap(),
                json!({"device_auth_id":DEVICE,"user_code":USER_CODE})
            );
            Reply::Json(json!({"authorization_code":CODE,"code_verifier":VERIFIER}))
        }
        "/oauth/token" => Reply::Json(tokens(request, expires, ACCESS, REFRESH)),
        "/.well-known/openid-configuration" => Reply::Json(
            json!({"issuer":identity::issuer(request),"jwks_uri":format!("{}/.well-known/jwks.json",identity::issuer(request))}),
        ),
        "/.well-known/jwks.json" => Reply::Json(identity::keys()),
        path if path.starts_with("/models") => catalog(
            request,
            &Options::defaults(Authentication::ChatGptOAuth).unwrap(),
            ACCESS,
        ),
        path if path.split('?').next() == Some("/v1/models") => {
            let options = Options::defaults(Authentication::ChatGptOAuth).unwrap();
            assert_eq!(request.method, "GET");
            assert!(request.body.is_empty());
            assert_eq!(
                request.path,
                format!(
                    "/v1/models?client_version={}",
                    options.catalog_version().unwrap()
                )
            );
            let access = if request.headers["authorization"] == format!("Bearer {ACCESS}") {
                ACCESS
            } else {
                REFRESHED
            };
            headers(request, &options, access);
            Reply::Raw(
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
            )
        }
        _ => panic!("unexpected Codex fixture request: {}", request.path),
    }
}

pub(crate) fn tokens(
    request: &discovery_support::Request,
    expires: u64,
    access: &str,
    refresh: &str,
) -> Value {
    let form = identity::form(request);
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.headers["content-type"],
        "application/x-www-form-urlencoded"
    );
    assert_eq!(form["client_id"], CLIENT);
    assert!(!form.contains_key("resource"));
    assert!(!form.contains_key("client_secret"));
    match form["grant_type"].as_str() {
        "authorization_code" => {
            assert_eq!(
                form["redirect_uri"],
                "https://auth.openai.com/deviceauth/callback"
            );
            assert_eq!(form["code"], CODE);
            assert_eq!(form["code_verifier"], VERIFIER);
            assert!(!form.contains_key("scope"));
        }
        "refresh_token" => {
            assert_eq!(form["refresh_token"], REFRESH);
            assert_eq!(form["scope"], "openid profile email");
            assert!(!form.contains_key("redirect_uri"));
            assert!(!form.contains_key("code"));
        }
        _ => panic!("unexpected Codex token grant"),
    }
    json!({"access_token":access,"refresh_token":refresh,"token_type":"Bearer","expires_in":expires,
        "id_token":identity::signed(&json!({"iss":identity::issuer(request),"aud":CLIENT,
            "sub":"isolated-codex-subject","email":"codex@example.com","iat":future_expiry()-3600,"exp":future_expiry(),
            "https://api.openai.com/auth":{"chatgpt_account_id":ACCOUNT}}))})
}

pub(super) fn headers(request: &discovery_support::Request, options: &Options, access: &str) {
    assert_eq!(request.headers["authorization"], format!("Bearer {access}"));
    assert_eq!(request.headers["chatgpt-account-id"], ACCOUNT);
    assert_eq!(request.headers["originator"], "codex_cli_rs");
    assert_eq!(
        request.headers["version"],
        options.catalog_version().unwrap()
    );
    assert_eq!(request.headers["user-agent"], options.user_agent());
}

pub(super) fn catalog(
    request: &discovery_support::Request,
    options: &Options,
    access: &str,
) -> Reply {
    assert_eq!(request.method, "GET");
    assert!(request.body.is_empty());
    assert_eq!(
        request.path,
        format!(
            "/models?client_version={}",
            options.catalog_version().unwrap()
        )
    );
    headers(request, options, access);
    Reply::Json(models())
}

pub(super) fn models() -> Value {
    json!({"models":[
        {"slug":"fixture","display_name":"Fixture","visibility":"list","context_window":4096,
            "max_output_tokens":1024,"input_modalities":["text","image"],
            "supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],"default_reasoning_level":"high"},
        {"slug":"unknown","display_name":"Unknown","visibility":"list"},
        {"slug":"hidden","display_name":"Hidden","visibility":"hidden"}
    ]})
}

pub(super) fn input(
    request: &discovery_support::Request,
    options: &Options,
    access: &str,
) -> Value {
    assert_eq!(request.path, "/responses");
    assert_eq!(request.method, "POST");
    headers(request, options, access);
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "fixture");
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    let session = body["prompt_cache_key"].as_str().unwrap();
    assert!(session.parse::<SessionId>().is_ok());
    assert_eq!(request.headers["session-id"], session);
    assert_eq!(request.headers["x-codex-routing-hint"], "model=fixture");
    assert!(body["instructions"].is_string());
    assert!(body.get("include").is_none());
    assert!(
        body["input"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["role"] != "system" && item.get("namespace").is_none())
    );
    let tools = body["tools"].as_array().unwrap();
    assert!(tools.iter().all(|tool| tool["type"] != "namespace"));
    assert!(tools.iter().any(|tool| tool["type"] == "function"));
    for field in [
        "max_output_tokens",
        "max_tool_calls",
        "temperature",
        "top_p",
        "previous_response_id",
        "prompt_cache_retention",
        "stream_options",
        "user",
    ] {
        assert!(body.get(field).is_none(), "unsupported Codex field {field}");
    }
    assert!(!request.body.contains(REFRESH));
    assert!(!request.body.contains(ROTATED));
    body
}

pub(super) fn message(text: &str) -> Value {
    json!({"type":"message","id":"codex-answer","role":"assistant","status":"completed",
        "content":[{"type":"output_text","text":text,"annotations":[]}]})
}

pub(super) fn completed(output: Value) -> Value {
    json!({"type":"response.completed","sequence_number":2,"response":{"id":"codex-response",
        "object":"response","created_at":1,"status":"completed","model":"fixture","output":output}})
}

pub(super) fn delta(text: &str) -> Value {
    json!({"type":"response.output_text.delta","sequence_number":1,"item_id":"codex-answer",
        "output_index":0,"content_index":0,"delta":text,"logprobs":[]})
}

pub(super) fn stream(mut events: Vec<Value>, done: bool) -> Reply {
    let response = RequestId::new().to_string();
    for event in &mut events {
        if event["response"].is_object() {
            event["response"]["id"] = json!(response);
        }
    }
    events.insert(
        0,
        json!({"type":"response.created","sequence_number":0,
        "response":{"id":response,"object":"response","created_at":1,
            "status":"in_progress","model":"fixture","output":[]}}),
    );
    let mut body =
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
            .to_string();
    for event in events {
        body.push_str(&format!("data: {event}\n\n"));
    }
    if done {
        body.push_str("data: [DONE]\n\n");
    }
    Reply::Raw(body)
}

pub(super) fn generation(
    request: &discovery_support::Request,
    options: &Options,
    access: &str,
) -> Reply {
    let body = input(request, options, access);
    let continuation = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .take_while(|item| item["role"] != "user")
        .any(|item| item["type"] == "function_call_output");
    if continuation {
        stream(
            vec![
                delta("Authorized Codex response"),
                completed(json!([message("Authorized Codex response")])),
            ],
            true,
        )
    } else {
        let digest = blake3::hash(b"files").to_hex();
        let tool = format!("plugin_{}_read_file", &digest[..16]);
        assert!(
            body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value["name"] == tool)
        );
        stream(
            vec![completed(
                json!([{"type":"function_call","id":"codex-call-item","call_id":"codex-call",
            "name":tool,"arguments":"{\"path\":\"fixture.txt\"}","status":"completed"}]),
            )],
            true,
        )
    }
}
