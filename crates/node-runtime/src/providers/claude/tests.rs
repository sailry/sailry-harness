use super::*;

#[test]
fn matches_bun_identity_and_checksum() {
    assert_eq!(
        xxhash_rust::xxh64::xxh64(b"omp parity fixture", SEED),
        0xc707a0ca4d9f252e
    );
    assert_eq!(
        metadata("install", "session", Some("account"))["user_id"],
        r#"{"device_id":"ba954c55410a7cce3187c0fc769f3735ac42a2a9b9e935a98a62034615c83c40","session_id":"session","account_uuid":"account"}"#
    );
}

#[test]
fn hashes_final_body_without_touching_user_text() {
    let input = json!({"model":"fixture","messages":[{"role":"user","content":"x-anthropic-billing-header: cc_version=fake; cch=00000"}],"system":"User system"});
    let bytes = encode(&input).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value["messages"][0]["content"][0]["text"],
        input["messages"][0]["content"]
    );
    assert_eq!(value["system"][1]["text"], IDENTITY);
    assert_eq!(value["system"][2]["text"], "User system");
    assert_eq!(value["tools"], json!([]));
    let text = value["system"][0]["text"].as_str().unwrap();
    let actual = text.split("cch=").nth(1).unwrap().trim_end_matches(';');
    assert_eq!(actual.len(), 5);
    let mut restored = value.clone();
    restored["system"][0]["text"] = json!(text.replace(&format!("cch={actual}"), "cch=00000"));
    let restored = serde_json::to_vec(&encoding::Body(&restored)).unwrap();
    assert_eq!(
        actual,
        format!(
            "{:05x}",
            xxhash_rust::xxh64::xxh64(&restored, SEED) & 0xfffff
        )
    );
}

#[test]
fn distinguishes_agent_and_utility_headers() {
    let utility = betas(&json!({"tools":[]}));
    assert!(utility.contains("structured-outputs-2025-12-15"));
    assert!(!utility.contains("claude-code-20250219"));
    let agent = betas(&json!({"tools":[{"name":"_read"}],"thinking":{"type":"adaptive"}}));
    assert!(agent.starts_with("claude-code-20250219,oauth-2025-04-20"));
    assert!(agent.ends_with("effort-2025-11-24,fallback-credit-2026-06-01"));
    assert!(!agent.contains("context-1m"));
}

#[test]
fn transforms_only_message_requests() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::new();
    let mut request = client
        .post("https://api.anthropic.com/v1/messages")
        .json(&json!({"messages":[{"role":"user","content":"Hello"}]}))
        .build()
        .unwrap();
    decorate(&mut request, "").unwrap();
    assert_eq!(request.url().query(), Some("beta=true"));
    assert_eq!(request.headers()["accept"], "application/json");
    assert_eq!(
        request.headers()["user-agent"],
        "claude-cli/2.1.280 (external, cli)"
    );
    assert_eq!(request.headers()["x-stainless-package-version"], "0.112.1");
    assert!(
        request.headers()["anthropic-beta"]
            .to_str()
            .unwrap()
            .contains("oauth-2025-04-20")
    );
    let mut catalog = client
        .get("https://api.anthropic.com/v1/models")
        .build()
        .unwrap();
    decorate(&mut catalog, "").unwrap();
    assert!(catalog.url().query().is_none());
    assert!(catalog.body().is_none());
}

#[test]
fn preserves_builtin_tools_and_first_text() {
    assert_eq!(tool_name("read_file"), "_read_file");
    assert_eq!(tool_name("_custom"), "__custom");
    assert_eq!(tool_name("web_search"), "web_search");
    assert_eq!(
        first_user(
            &json!({"messages":[{"role":"user","content":[{"type":"text","text":" "},{"type":"text","text":"First"},{"type":"text","text":"Second"}]}]})
        ),
        "First"
    );
}

#[test]
fn refreshes_default_version_and_preserves_overrides() {
    assert_eq!(
        agent(
            sailry_protocol::conversation::oauth::DEFAULT_CLAUDE_AGENT,
            "2.1.300"
        ),
        "claude-cli/2.1.300 (external, cli)"
    );
    assert_eq!(
        agent("claude-cli/2.1.280", "2.1.300"),
        "claude-cli/2.1.300 (external, cli)"
    );
    assert_eq!(
        agent("claude-cli/2.1.290 custom", "2.1.300"),
        "claude-cli/2.1.290 custom"
    );
    let body = json!({"messages":[{"role":"user","content":"Hello"}],"sailry_claude_first_user":"Original"});
    let encoded = encode_version(&body, "2.1.300").unwrap();
    let body: Value = serde_json::from_slice(&encoded).unwrap();
    assert!(body.get("sailry_claude_first_user").is_none());
    assert!(
        body["system"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("x-anthropic-billing-header: cc_version=2.1.300.")
    );
}

#[test]
fn omits_empty_account_identity() {
    assert_eq!(
        metadata("install", "session", Some("")),
        metadata("install", "session", None)
    );
}

#[test]
fn freezes_version_for_headers_and_body() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut request = reqwest::Client::new()
        .post("https://api.anthropic.com/v1/messages")
        .json(&json!({"messages":[],"sailry_claude_version":"2.1.300"}))
        .build()
        .unwrap();
    decorate(&mut request, "claude-cli/2.1.280").unwrap();
    assert_eq!(
        request.headers()["user-agent"],
        "claude-cli/2.1.300 (external, cli)"
    );
    let body: Value = serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
    assert!(body.get("sailry_claude_version").is_none());
    assert!(
        body["system"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("x-anthropic-billing-header: cc_version=2.1.300.")
    );
}
