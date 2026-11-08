use super::*;

fn source(result: Value) -> Raw {
    let mut raw = Raw::new(Some("call".into()), "probe".into(), json!({}));
    raw.format = ToolFormat::Mcp;
    raw.failure = Some(Failure::tool(None));
    raw.result = Some(result);
    raw
}

#[test]
fn extracts_envelope_messages_without_changing_results() {
    for (envelope, expected) in [
        (
            json!({"error":{"code":"APP_NOT_FOUND","message":"No running application matches 'Terminal'"},"message":"outer","suggestion":"Open it first"}),
            Some("No running application matches 'Terminal'"),
        ),
        (
            json!({"error":"other","message":"Top-level message"}),
            Some("Top-level message"),
        ),
        (json!({"error":"Plain failure"}), Some("Plain failure")),
        (
            json!({"error":{"code":"APP_NOT_FOUND"},"id":"internal-id"}),
            None,
        ),
    ] {
        let result =
            json!({"isError":true,"content":[{"type":"text","text":envelope.to_string()}]});
        let raw = source(result.clone());
        let summary = raw.summary();
        assert_eq!(summary.error.as_deref(), expected);
        assert_eq!(summary.has_details, expected.is_none());
        assert_eq!(raw.detail().result, Some(result));
    }
}

#[test]
fn retains_plain_text_and_unicode_spacing() {
    let result = json!({"isError":true,"content":[
        {"type":"text","text":"  参数错误 🙂"},
        {"type":"image","mimeType":"image/png","data":"ignored"},
        {"type":"text","text":"Try another value  "}
    ]});
    let raw = source(result.clone());
    assert_eq!(
        raw.summary().error.as_deref(),
        Some("  参数错误 🙂\nTry another value  ")
    );
    assert_eq!(raw.detail().result, Some(result));
    for message in ["Malformed {\"error\":", "Ordinary failure", " \n "] {
        let raw = source(json!({"isError":true,"content":[{"type":"text","text":message}]}));
        assert_eq!(raw.summary().error.as_deref(), Some(message));
    }
}

#[test]
fn prioritizes_diagnostics_and_structured_messages() {
    let mut raw = source(json!({"isError":true,"structuredContent":{
        "error":{"message":"Structured failure"},
        "sailry_result":{"version":1,"diagnostics":[
            {"text":"First diagnostic","error":true},{"text":"Second diagnostic","error":true}
        ]}
    },"content":[{"type":"text","text":"MCP fallback"}]}));
    for observed in [false, true] {
        raw.failure = observed.then(|| Failure::tool(None));
        assert_eq!(
            raw.summary().error.as_deref(),
            Some("First diagnostic\nSecond diagnostic")
        );
    }
    raw.result.as_mut().unwrap()["structuredContent"]["sailry_result"]["version"] = json!(2);
    assert_eq!(raw.summary().error.as_deref(), Some("Structured failure"));
    raw.result.as_mut().unwrap()["structuredContent"]["error"] = json!("Plain structured failure");
    assert_eq!(
        raw.summary().error.as_deref(),
        Some("Plain structured failure")
    );
}

#[test]
fn retains_rpc_messages_and_captured_revisions() {
    let result =
        json!({"code":-32602,"message":"Invalid parameters","data":{"suggestion":"Preserved"}});
    let mut raw = source(result.clone());
    raw.format = ToolFormat::McpError;
    assert_eq!(raw.summary().error.as_deref(), Some("Invalid parameters"));
    let revision = raw.summary().revision;
    raw.error = Some("Original server message".into());
    assert_eq!(
        raw.summary().error.as_deref(),
        Some("Original server message")
    );
    assert_ne!(raw.summary().revision, revision);
    assert_eq!(raw.detail().result, Some(result));
}

#[test]
fn bounds_headlines_and_retains_long_details() {
    let message = "参数🙂 ".repeat(400);
    let result = json!({"isError":true,"content":[{"type":"text","text":json!({"error":{"message":message}}).to_string()}]});
    let raw = source(result.clone());
    let summary = raw.summary();
    assert_eq!(
        summary.error.unwrap(),
        message.chars().take(1024).collect::<String>()
    );
    assert!(summary.has_details);
    assert_eq!(raw.detail().result, Some(result));
}
