use super::*;
use crate::{agent::model::names::Exposure, plugins::tools::alias};

#[test]
fn carries_exact_tool_identity_and_unchanged_bodies() {
    let canonical = alias("files", "read_file");
    let names = Exposure::new(
        [(canonical.clone(), "read_file".into())],
        [canonical.clone()].into(),
        false,
    );
    let messages = [
        Message::user().with_text(canonical.clone()),
        Message::assistant().with_tool_request(
            "file",
            Ok(CallToolRequestParams::new(canonical.clone())
                .with_arguments(serde_json::from_value(json!({"path":canonical})).unwrap())),
        ),
        Message::user().with_tool_response(
            "file",
            Ok(CallToolResult::success(vec![Content::text(
                canonical.clone(),
            )])),
        ),
    ];
    let tools = [Tool::new(
        canonical.clone(),
        canonical.clone(),
        serde_json::Map::new(),
    )];
    let projected = names.messages(&messages);
    let tools = names.tools(&tools);
    for dialect in [Dialect::Generic, Dialect::OpenAi, Dialect::Gemini] {
        let wire = request::convert(
            &ModelConfig::new("fixture"),
            &canonical,
            &projected,
            &tools,
            request::Context {
                endpoint: "https://fixture.invalid",
                session: "fixture",
                effort: Effort::Default,
                features: Default::default(),
                capabilities: &capabilities(),
                dialect,
            },
        )
        .unwrap();
        assert_eq!(wire.system.as_deref(), Some(canonical.as_str()));
        assert_eq!(wire.tools[0].name, "read_file");
        assert_eq!(
            wire.tools[0].description.as_deref(),
            Some(canonical.as_str())
        );
        let value = serde_json::to_value(&wire).unwrap();
        let text = value.to_string();
        assert!(text.contains(&canonical));
        let mut calls = 0;
        let mut results = 0;
        for message in wire.messages {
            match message {
                wire::Message::Assistant { parts } => {
                    for part in parts {
                        if let wire::Part::ToolCall {
                            id,
                            name,
                            arguments,
                        } = part
                        {
                            assert_eq!(id, "file");
                            assert_eq!(name, "read_file");
                            assert_eq!(arguments["path"], canonical);
                            calls += 1;
                        }
                    }
                }
                wire::Message::Tool {
                    call_id,
                    name,
                    parts,
                    ..
                } => {
                    assert_eq!(call_id, "file");
                    assert_eq!(name, "read_file");
                    let [wire::Part::Json { value }] = parts.as_slice() else {
                        panic!("tool JSON expected")
                    };
                    assert_eq!(value["content"][0]["text"], canonical);
                    results += 1;
                }
                _ => {}
            }
        }
        assert_eq!((calls, results), (1, 1));
    }
}

#[test]
fn pairs_reused_ids_with_current_owner() {
    let old = alias("previous", "read");
    let current = alias("current", "read");
    let names = Exposure::new(
        [(current.clone(), "read".into())],
        [current.clone()].into(),
        false,
    );
    let messages = [
        Message::assistant()
            .with_tool_request("reused", Ok(CallToolRequestParams::new(old.clone()))),
        Message::user().with_tool_response(
            "reused",
            Ok(CallToolResult::success(vec![Content::text(
                "previous result",
            )])),
        ),
        Message::user().with_text(old.clone()),
        Message::assistant()
            .with_tool_request("reused", Ok(CallToolRequestParams::new(current.clone()))),
        Message::user().with_tool_response(
            "reused",
            Ok(CallToolResult::success(vec![Content::text(
                "current result",
            )])),
        ),
    ];
    let projected = names.messages(&messages);
    let wire = request::convert(
        &ModelConfig::new("fixture"),
        "",
        &projected,
        &[],
        request::Context {
            endpoint: "https://fixture.invalid",
            session: "fixture",
            effort: Effort::Default,
            features: Default::default(),
            capabilities: &capabilities(),
            dialect: Dialect::Generic,
        },
    )
    .unwrap();
    let actual: Vec<_> = wire
        .messages
        .iter()
        .filter_map(|message| {
            let wire::Message::Tool { call_id, name, .. } = message else {
                return None;
            };
            assert_eq!(call_id, "reused");
            Some(name.as_str())
        })
        .collect();
    assert_eq!(actual, [old.as_str(), "read"]);
    let MessageContentBlock::ToolRequest(request) = &messages[3].content[0] else {
        unreachable!()
    };
    assert_eq!(request.tool_call.as_ref().unwrap().name, current);
}
