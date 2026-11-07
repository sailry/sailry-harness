use super::*;
use crate::providers::request::fixture;
use serde_json::json;

fn tool(name: &str) -> Tool {
    rmcp::model::Tool::new(name.to_owned(), "", serde_json::Map::new()).into()
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
fn matches_captured_request_baselines() {
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
        let messages = vec![
            Message::assistant()
                .with_tool_request("call", Ok(rmcp::model::CallToolRequestParams::new("read"))),
            Message::assistant().with_tool_request(
                "historical",
                Ok(rmcp::model::CallToolRequestParams::new("READ")),
            ),
        ];
        let mut request = Request {
            model: "fixture-model".into(),
            max_tokens: Some(64),
            temperature: Some(0.25),
            params: [("captured_parameter".into(), json!(7))]
                .into_iter()
                .collect(),
            headers: [(
                reqwest::header::HeaderName::from_static("x-captured-header"),
                reqwest::header::HeaderValue::from_static("kept"),
            )]
            .into_iter()
            .collect(),
            system: "Captured system".into(),
            messages,
            tools: vec![tool("read"), tool("custom")],
            ..Request::default()
        };
        apply(&prepared, &mut request);
        let history: Vec<_> = request
            .messages
            .iter()
            .flat_map(|message| &message.content)
            .filter_map(|block| {
                if let Content::ToolCall(call) = block {
                    Some(call.name.as_str())
                } else {
                    None
                }
            })
            .collect();
        let actual = json!({
            "max_tokens":request.max_tokens, "temperature":request.temperature,
            "parameters":request.params, "headers":request.headers.iter().map(|(key,value)| (key.as_str(),value.to_str().unwrap())).collect::<std::collections::BTreeMap<_,_>>(),
            "system":request.system, "tools":request.tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>(),
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
                expected["parameters"]["store"] = json!(false);
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
    let messages = [Message::assistant().with_text("retired").with_tool_request(
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
    let mut request = Request {
        model: "fixture-model".into(),
        messages: messages.to_vec(),
        ..Request::default()
    };
    apply(&prepared, &mut request);
    let Content::ToolCall(call) = &request.messages[0].content[1] else {
        panic!("tool request expected")
    };
    assert_eq!(call.name, "_retired");
    let Content::Text { text } = &request.messages[0].content[0] else {
        panic!("text expected")
    };
    assert_eq!(text, "retired");
}
