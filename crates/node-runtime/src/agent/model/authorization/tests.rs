use super::*;
use crate::providers::request::fixture;
use rmcp::model::Tool;
use serde_json::json;

fn tool(name: &str) -> Tool {
    Tool::new(name.to_owned(), "", serde_json::Map::new())
}

#[test]
fn reverses_only_exact_wire_names() {
    let names = policy::Names::new(&["read", "custom"]).unwrap().reverse;
    assert_eq!(names.get("_read").map(String::as_str), Some("read"));
    assert_eq!(names.get("_custom").map(String::as_str), Some("custom"));
    assert!(!names.contains_key("READ"));
    assert!(!names.contains_key("CUSTOM"));
}

#[test]
fn rejects_duplicate_names() {
    assert!(policy::Names::new(&["read", "Read"]).is_ok());
    assert!(policy::Names::new(&["custom", "custom"]).is_err());
}

#[test]
fn matches_captured_gdk_request_baselines() {
    // Frozen pre-refactor stream inputs; GDK's unchanged formatter owns the wire body.
    for authentication in [
        Authentication::ApiKey,
        Authentication::ChatGpt,
        Authentication::ChatGptOAuth,
        Authentication::ClaudeOAuth,
        Authentication::Copilot,
        Authentication::Plugin,
    ] {
        let api = if authentication == Authentication::ClaudeOAuth {
            ModelApi::Anthropic
        } else {
            ModelApi::Responses
        };
        let provider = fixture::provider(authentication, api);
        let credentials = fixture::credentials(authentication);
        let prepared = policy::prepare(
            &provider,
            &credentials,
            fixture::metadata("fixture-model", "Captured system", &["read", "custom"]),
        )
        .unwrap();
        let mut config = ModelConfig::new("fixture-model").with_max_tokens(Some(64));
        config.temperature = Some(0.25);
        config.request_params = Some([("captured_parameter".into(), json!(7))].into());
        config.request_headers = Some([("x-captured-header".into(), "kept".into())].into());
        let mut system = "Captured system".to_owned();
        let mut messages = vec![
            Message::assistant()
                .with_tool_request("call", Ok(rmcp::model::CallToolRequestParams::new("read"))),
            Message::assistant().with_tool_request(
                "historical",
                Ok(rmcp::model::CallToolRequestParams::new("READ")),
            ),
        ];
        let mut tools = vec![tool("read"), tool("custom")];
        apply(
            &prepared,
            &mut config,
            &mut system,
            &mut messages,
            &mut tools,
        )
        .unwrap();
        let history: Vec<_> = messages
            .iter()
            .flat_map(|message| &message.content)
            .filter_map(|block| {
                block
                    .as_tool_request()
                    .and_then(|request| request.tool_call.as_ref().ok())
                    .map(|call| call.name.as_ref())
            })
            .collect();
        let actual = json!({
            "max_tokens":config.max_tokens, "temperature":config.temperature,
            "parameters":config.request_params, "headers":config.request_headers,
            "system":system, "tools":tools.iter().map(|tool| tool.name.as_ref()).collect::<Vec<_>>(),
            "history":history,
        });
        let mut headers = fixture::headers(authentication);
        headers["x-captured-header"] = json!("kept");
        let mut expected = json!({
            "max_tokens":64, "temperature":0.25, "parameters":{"captured_parameter":7}, "headers":headers,
            "system":"Captured system", "tools":["read","custom"], "history":["read","READ"]
        });
        match authentication {
            Authentication::ChatGpt => {
                expected["max_tokens"] = serde_json::Value::Null;
                expected["temperature"] = serde_json::Value::Null;
            }
            Authentication::ChatGptOAuth => {
                expected["max_tokens"] = serde_json::Value::Null;
                expected["temperature"] = serde_json::Value::Null;
                expected["parameters"]["instructions"] = json!("Captured system");
                expected["parameters"]["prompt_cache_key"] = json!(fixture::SESSION);
                expected["system"] = json!("");
            }
            Authentication::ClaudeOAuth => {
                expected["parameters"]["metadata"] = crate::providers::claude::metadata(
                    "33333333-3333-4333-8333-333333333333",
                    fixture::SESSION,
                    None,
                );
                expected["tools"] = json!(["_read", "_custom"]);
                expected["history"] = json!(["_read", "READ"]);
            }
            _ => {}
        }
        assert_eq!(actual, expected);
    }
}

#[test]
fn prefixes_retired_tools_without_rewriting_content() {
    let tools = [tool("current")];
    let mut messages = [Message::assistant().with_text("retired").with_tool_request(
        "old",
        Ok(rmcp::model::CallToolRequestParams::new("retired")),
    )];
    assert_eq!(request_names(&tools, &messages, false), vec!["current"]);
    let names = request_names(&tools, &messages, true);
    assert_eq!(names, vec!["current", "retired"]);
    let provider = fixture::provider(Authentication::ClaudeOAuth, ModelApi::Anthropic);
    let prepared = policy::prepare(
        &provider,
        &fixture::credentials(Authentication::ClaudeOAuth),
        fixture::metadata("fixture-model", "Captured system", &names),
    )
    .unwrap();
    apply(
        &prepared,
        &mut ModelConfig::new("fixture-model"),
        &mut String::new(),
        &mut messages,
        &mut [],
    )
    .unwrap();
    let MessageContentBlock::ToolRequest(request) = &messages[0].content[1] else {
        panic!("tool request expected")
    };
    assert_eq!(request.tool_call.as_ref().unwrap().name, "_retired");
    let MessageContentBlock::Text(text) = &messages[0].content[0] else {
        panic!("text expected")
    };
    assert_eq!(text.text, "retired");
}
