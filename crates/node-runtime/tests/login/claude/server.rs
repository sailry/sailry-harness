use super::*;
use sha2::{Digest, Sha256};

pub(super) fn authorization(url: &str, endpoint: &str, redirect: Option<&str>) {
    let url = url::Url::parse(url).unwrap();
    assert_eq!(url.origin().ascii_serialization(), endpoint);
    assert_eq!(url.path(), "/oauth/authorize");
    let fields = url
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(fields["client_id"], CLIENT);
    assert_eq!(fields["response_type"], "code");
    assert_eq!(fields["code"], "true");
    assert_eq!(fields["redirect_uri"], redirect.unwrap_or(COPY_REDIRECT));
    assert_eq!(fields["code_challenge_method"], "S256");
    assert_eq!(
        fields["scope"],
        "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload"
    );
    assert!(fields["state"].len() >= 43);
    let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(fields["state"].as_bytes()));
    assert!(fields["code_challenge"] == expected);
}

pub(crate) fn tokens(request: &discovery_support::Request, fresh: bool, expires: u64) -> Value {
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/oauth/token");
    assert!(request.headers["content-type"].starts_with("application/json"));
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["client_id"], CLIENT);
    assert!(body.get("scope").is_none());
    if fresh {
        assert_eq!(body["grant_type"], "refresh_token");
        assert!(body["refresh_token"] == REFRESH);
        assert_eq!(body.as_object().unwrap().len(), 3);
    } else {
        assert_eq!(body["grant_type"], "authorization_code");
        assert!(body["code"] == CODE);
        assert!(body["code_verifier"] == body["state"]);
        assert!(
            body["state"]
                .as_str()
                .is_some_and(|state| state.len() >= 43)
        );
        assert!(matches!(
            body["redirect_uri"].as_str(),
            Some(REDIRECT | COPY_REDIRECT)
        ));
        assert_eq!(body.as_object().unwrap().len(), 6);
    }
    json!({
        "access_token": if fresh { REFRESHED } else { ACCESS },
        "refresh_token": if fresh { ROTATED } else { REFRESH },
        "expires_in": expires,
    })
}

pub(super) fn headers(request: &discovery_support::Request, access: &str, agent: &str) {
    assert!(request.headers["authorization"] == format!("Bearer {access}"));
    assert!(!request.headers.contains_key("x-api-key"));
    assert_eq!(request.headers["user-agent"], agent);
    assert_eq!(request.headers["anthropic-version"], "2023-06-01");
    let betas = request.headers["anthropic-beta"]
        .split(',')
        .collect::<Vec<_>>();
    assert!(betas.contains(&"claude-code-20250219"));
    assert!(betas.contains(&"oauth-2025-04-20"));
    assert_eq!(
        request.headers["anthropic-dangerous-direct-browser-access"],
        "true"
    );
    assert_eq!(request.headers["x-app"], "cli");
}

pub(super) fn catalog(request: &discovery_support::Request, access: &str, agent: &str) -> Reply {
    headers(request, access, agent);
    assert_eq!(request.method, "GET");
    assert!(request.body.is_empty());
    let url = url::Url::parse(&format!("http://fixture{}", request.path)).unwrap();
    assert_eq!(url.path(), "/v1/models");
    let fields = url
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(fields["limit"], "100");
    Reply::Json(if let Some(cursor) = fields.get("after_id") {
        assert_eq!(cursor.as_ref(), "fixture");
        json!({"data":[{
            "id":"unknown", "type":"model", "display_name":"Unknown fixture",
            "max_input_tokens":16384, "max_tokens":1024, "tool_call":true, "reasoning":false
        }], "has_more":false, "last_id":"unknown"})
    } else {
        json!({"data":[{
            "id":"fixture", "type":"model", "display_name":"Claude fixture",
            "max_input_tokens":8192, "max_tokens":512, "tool_call":true, "reasoning":false,
            "vision":true
        }], "has_more":true, "last_id":"fixture"})
    })
}

pub(crate) fn standard(request: &discovery_support::Request) -> Reply {
    if request.path == "/v1/oauth/token" {
        Reply::Json(tokens(request, false, 3600))
    } else if request.path.starts_with("/v1/models") {
        catalog(request, ACCESS, "claude-cli/2.1.280")
    } else {
        panic!("unexpected Claude fixture endpoint")
    }
}

pub(super) fn generation(request: &discovery_support::Request, access: &str, tools: bool) -> Reply {
    headers(request, access, "claude-cli/2.1.280");
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/messages");
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "fixture");
    assert_eq!(body["stream"], true);
    assert_eq!(body["max_tokens"], 512);
    for tool in body["tools"].as_array().into_iter().flatten() {
        if tool["name"] == "manage_mcp" {
            let schema = &tool["input_schema"];
            assert_eq!(schema["type"], "object");
            assert!(schema["oneOf"].is_array());
        }
    }
    let system = body["system"].as_array().unwrap();
    assert!(system[0]["text"].as_str().unwrap().starts_with(
        "You are Claude Code, Anthropic's official CLI for Claude.\n\nYou are Sailry,"
    ));
    assert_eq!(
        system
            .iter()
            .filter_map(|block| block["text"].as_str())
            .map(|text| text.matches(INSTRUCTION).count())
            .sum::<usize>(),
        1
    );
    assert_eq!(
        body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "user")
            .flat_map(|message| message["content"].as_array().unwrap())
            .filter_map(|block| block["text"].as_str())
            .map(|text| text.matches(INSTRUCTION).count())
            .sum::<usize>(),
        0
    );
    let task = crate::agent_support::task_request(&body);
    let continuation = task["messages"]
        .as_array()
        .unwrap()
        .last()
        .is_some_and(|message| {
            message["content"]
                .as_array()
                .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "tool_result"))
        });
    let call = tools && !continuation;
    let content = if call {
        let name = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|tool| tool["name"].as_str().filter(|name| *name == "read_file"))
            .expect("files read tool expected");
        json!({"type":"tool_use", "id":"call_claude", "name":name, "input":{}})
    } else {
        if tools {
            assert!(request.body.contains("Isolated tool content"));
        }
        json!({"type":"text", "text":""})
    };
    let delta = if call {
        json!({"type":"input_json_delta", "partial_json":"{\"path\":\"fixture.txt\"}"})
    } else {
        json!({"type":"text_delta", "text":ANSWER})
    };
    let events = [
        json!({"type":"message_start", "message":{
            "id":format!("msg_claude_{}",RequestId::new()), "type":"message", "role":"assistant", "model":"fixture",
            "content":[], "stop_reason":null, "stop_sequence":null,
            "usage":{"input_tokens":16, "output_tokens":0, "cache_read_input_tokens":4, "cache_creation_input_tokens":3}
        }}),
        json!({"type":"content_block_start", "index":0, "content_block":content}),
        json!({"type":"content_block_delta", "index":0, "delta":delta}),
        json!({"type":"content_block_stop", "index":0}),
        json!({"type":"message_delta", "delta":{
            "stop_reason":if call { "tool_use" } else { "end_turn" }, "stop_sequence":null
        }, "usage":{"output_tokens":2}}),
        json!({"type":"message_stop"}),
    ];
    let events = events
        .into_iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect::<String>();
    Reply::Raw(format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{events}"
    ))
}
