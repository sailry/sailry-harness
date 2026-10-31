//! Claude OAuth wire contract from oh-my-pi c29f7c56 (MIT; see LICENSE.omp).
use reqwest::{
    Request,
    header::{HeaderMap, HeaderValue},
};
use sailry_protocol::Fault;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

mod cache;
mod encoding;
pub(crate) mod version;
const SDK: &str = "0.112.1";
const IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";
const BILLING: &str = "x-anthropic-billing-header:";
const SEED: u64 = 0x4d659218e32a3268;

#[cfg(test)]
mod tests;

pub(crate) fn user_agent(configured: &str) -> String {
    agent(configured, &version::current())
}

fn agent(configured: &str, version: &str) -> String {
    if configured.to_ascii_lowercase().starts_with("claude-cli")
        && configured != sailry_protocol::conversation::oauth::DEFAULT_CLAUDE_AGENT
        && configured != "claude-cli/2.1.280"
    {
        configured.into()
    } else {
        format!("claude-cli/{version} (external, cli)")
    }
}

pub(crate) fn headers(headers: &mut HeaderMap) {
    let os = match std::env::consts::OS {
        "macos" => "MacOS",
        "windows" => "Windows",
        "linux" => "Linux",
        "freebsd" => "FreeBSD",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        other => other,
    };
    for (key, value) in [
        ("accept", "application/json"),
        ("content-type", "application/json"),
        ("anthropic-version", "2023-06-01"),
        ("anthropic-dangerous-direct-browser-access", "true"),
        ("x-app", "cli"),
        ("connection", "keep-alive"),
        ("accept-encoding", "gzip, deflate, br, zstd"),
        ("x-stainless-arch", arch),
        ("x-stainless-lang", "js"),
        ("x-stainless-os", os),
        ("x-stainless-package-version", SDK),
        ("x-stainless-retry-count", "0"),
        ("x-stainless-runtime", "node"),
        ("x-stainless-runtime-version", "v26.3.0"),
        ("x-stainless-timeout", "600"),
    ] {
        headers.insert(
            reqwest::header::HeaderName::from_static(key),
            HeaderValue::from_str(value).expect("static Claude header"),
        );
    }
}

pub(crate) fn tool_name(name: &str) -> String {
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "web_search" | "code_execution" | "text_editor" | "computer"
    ) {
        name.into()
    } else {
        format!("_{name}")
    }
}

pub(crate) fn metadata(installation: &str, session: &str, account: Option<&str>) -> Value {
    let account = account.filter(|value| !value.is_empty());
    let mut hash = Sha256::new();
    if let Some(account) = account {
        hash.update(b"omp-claude-device-id-v2\0");
        hash.update(installation);
        hash.update(b"\0");
        hash.update(account);
    } else {
        hash.update(b"omp-claude-device-id-v1:");
        hash.update(installation);
    }
    // Preserve OMP's nested JSON string field order.
    #[derive(serde::Serialize)]
    struct Identity<'a> {
        device_id: String,
        session_id: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        account_uuid: Option<&'a str>,
    }
    json!({"user_id":serde_json::to_string(&Identity { device_id:format!("{:x}",hash.finalize()), session_id:session, account_uuid:account }).expect("Claude identity serialization")})
}

fn first_user(body: &Value) -> String {
    for message in body["messages"].as_array().into_iter().flatten() {
        if !matches!(message["role"].as_str(), Some("user" | "developer")) {
            continue;
        }
        match &message["content"] {
            Value::String(text) if !text.trim().is_empty() => return text.clone(),
            Value::Array(parts) => {
                if let Some(text) = parts
                    .iter()
                    .filter(|part| part["type"] == "text")
                    .filter_map(|part| part["text"].as_str())
                    .find(|text| !text.trim().is_empty())
                {
                    return text.into();
                }
                if parts.iter().any(|part| part["type"] == "image") {
                    return String::new();
                }
            }
            _ => {}
        }
    }
    String::new()
}

fn betas(body: &Value) -> String {
    let thinking = matches!(
        body["thinking"]["type"].as_str(),
        Some("enabled" | "adaptive")
    );
    let agent = thinking
        || body["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty());
    let mut betas = Vec::new();
    if agent {
        betas.push("claude-code-20250219");
    }
    betas.extend([
        "oauth-2025-04-20",
        "interleaved-thinking-2025-05-14",
        "thinking-token-count-2026-05-13",
        "context-management-2025-06-27",
        "prompt-caching-scope-2026-01-05",
    ]);
    if agent {
        betas.push("mid-conversation-system-2026-04-07");
        if thinking {
            betas.push("effort-2025-11-24");
        }
        betas.push("fallback-credit-2026-06-01");
    } else {
        betas.push("structured-outputs-2025-12-15");
    }
    betas.join(",")
}

fn system(body: &mut Value, version: &str) {
    let first = body
        .as_object_mut()
        .and_then(|body| body.remove("sailry_claude_first_user"))
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| first_user(body));
    let units: Vec<_> = first.encode_utf16().collect();
    let sampled = String::from_utf16_lossy(
        &[4, 7, 20].map(|index| units.get(index).copied().unwrap_or(b'0' as u16)),
    );
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(format!("59cf53e54c78{sampled}{version}"))
    );
    let mut blocks = match body.get("system") {
        Some(Value::Array(blocks)) => blocks.clone(),
        Some(Value::String(text)) if !text.is_empty() => vec![json!({"type":"text","text":text})],
        _ => vec![],
    };
    blocks.retain(|block| {
        !block["text"]
            .as_str()
            .is_some_and(|text| text.starts_with(BILLING) || text == IDENTITY)
    });
    let mut system = vec![
        json!({"type":"text","text":format!("{BILLING} cc_version={version}.{}; cc_entrypoint=cli; cch=00000;", &fingerprint[..3])}),
        json!({"type":"text","text":IDENTITY,"cache_control":{"type":"ephemeral"}}),
    ];
    system.extend(blocks);
    body["system"] = Value::Array(system);
    if body.get("tools").is_none() {
        body["tools"] = json!([]);
    }
    if matches!(
        body["thinking"]["type"].as_str(),
        Some("enabled" | "adaptive")
    ) && body.get("context_management").is_none()
    {
        body["context_management"] =
            json!({"edits":[{"type":"clear_thinking_20251015","keep":"all"}]});
    }
}

#[cfg(test)]
pub(crate) fn encode(body: &Value) -> Result<Vec<u8>, Fault> {
    encode_version(body, &version::current())
}

fn encode_version(body: &Value, version: &str) -> Result<Vec<u8>, Fault> {
    let mut body = body.clone();
    system(&mut body, version);
    if let Some(tools) = body["tools"].as_array_mut() {
        for tool in tools {
            if tool.get("type").is_none() || tool["type"] == "custom" {
                tool["eager_input_streaming"] = json!(tool["name"] != "_yield");
            }
        }
    }
    cache::apply(&mut body);
    let mut bytes = serde_json::to_vec(&encoding::Body(&body))
        .map_err(|_| Fault::invalid("Claude request serialization failed"))?;
    // Locate only the injected system block, never a user-authored placeholder.
    let marker =
        b"\"system\":[{\"type\":\"text\",\"text\":\"x-anthropic-billing-header: cc_version=";
    let start = bytes
        .windows(marker.len())
        .position(|value| value == marker)
        .ok_or_else(|| Fault::invalid("Claude billing block is missing"))?;
    let offset = bytes[start..]
        .windows(9)
        .position(|value| value == b"cch=00000")
        .map(|offset| start + offset + 4)
        .ok_or_else(|| Fault::invalid("Claude billing checksum is missing"))?;
    let checksum = format!("{:05x}", xxhash_rust::xxh64::xxh64(&bytes, SEED) & 0xfffff);
    bytes[offset..offset + 5].copy_from_slice(checksum.as_bytes());
    Ok(bytes)
}

pub(crate) fn decorate(request: &mut Request, configured: &str) -> Result<(), Fault> {
    if !request.url().path().ends_with("/v1/messages") {
        return Ok(());
    }
    let bytes = request
        .body()
        .and_then(reqwest::Body::as_bytes)
        .ok_or_else(|| Fault::invalid("Claude requires a buffered JSON request"))?;
    let mut body: Value = serde_json::from_slice(bytes)
        .map_err(|_| Fault::invalid("Claude request JSON is invalid"))?;
    let version = body
        .as_object_mut()
        .and_then(|body| body.remove("sailry_claude_version"))
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(version::current);
    let bytes = encode_version(&body, &version)?;
    let agent = agent(configured, &version);
    headers(request.headers_mut());
    request.headers_mut().insert(
        "user-agent",
        HeaderValue::from_str(&agent)
            .map_err(|_| Fault::invalid("Claude user agent is invalid"))?,
    );
    request.headers_mut().insert(
        "anthropic-beta",
        HeaderValue::from_str(&betas(&body)).expect("static Claude beta headers"),
    );
    request
        .headers_mut()
        .remove(reqwest::header::CONTENT_LENGTH);
    if !request.url().query_pairs().any(|(key, _)| key == "beta") {
        request
            .url_mut()
            .query_pairs_mut()
            .append_pair("beta", "true");
    }
    *request.body_mut() = Some(bytes.into());
    Ok(())
}
