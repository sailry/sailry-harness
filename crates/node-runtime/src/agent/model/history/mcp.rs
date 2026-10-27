//! Omit duplicate MCP text only from outgoing model requests.
#[cfg(test)]
use adk_core::{Content, Part};
use serde_json::Value;

#[cfg(test)]
fn project(contents: &mut [Content]) {
    for content in contents {
        for part in &mut content.parts {
            let Part::FunctionResponse {
                function_response, ..
            } = part
            else {
                continue;
            };
            if !function_response.name.starts_with("mcp_") {
                continue;
            }
            if let Some(value) = deduplicate(&function_response.response) {
                function_response.response = value;
            }
        }
    }
}

pub(super) fn deduplicate(value: &Value) -> Option<Value> {
    let (Some(output), Some(text)) = (
        value.get("output"),
        value.get("text").and_then(Value::as_array),
    ) else {
        return None;
    };
    let retained: Vec<_> = text
        .iter()
        .filter(|text| {
            text.as_str()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .as_ref()
                != Some(output)
        })
        .cloned()
        .collect();
    if retained.len() == text.len() {
        return None;
    }
    let mut value = value.clone();
    let object = value.as_object_mut().expect("MCP result has named fields");
    if retained.is_empty() {
        object.remove("text");
    } else {
        object.insert("text".into(), Value::Array(retained));
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use adk_core::FunctionResponseData;
    use serde_json::json;

    fn response(name: &str, value: Value) -> Content {
        Content {
            role: "function".into(),
            parts: vec![Part::FunctionResponse {
                function_response: FunctionResponseData::with_inline_data(
                    name,
                    value,
                    vec![adk_core::InlineDataPart {
                        mime_type: "image/png".into(),
                        data: vec![1, 2, 3],
                        uri: None,
                        annotations: Some(json!({"fixture":"image"})),
                    }],
                ),
                id: Some("call".into()),
                annotations: Some(json!({"fixture":"preserved"})),
            }],
        }
    }

    fn value(content: &Content) -> &Value {
        let Part::FunctionResponse {
            function_response, ..
        } = &content.parts[0]
        else {
            panic!("function response expected")
        };
        &function_response.response
    }

    fn encoded(contents: &[Content]) -> Value {
        serde_json::to_value(contents).unwrap()
    }

    #[test]
    fn projects_equivalent_json_without_changing_history() {
        let output = json!({"logs":"Complete log 中文 🙂\nwith whitespace", "status":"failed"});
        let canonical = vec![response(
            "mcp_fixture_logs",
            json!({"output":output,"text":[serde_json::to_string_pretty(&output).unwrap()]}),
        )];
        let mut request = canonical.clone();
        project(&mut request);
        assert_eq!(value(&request[0]), &json!({"output":output}));
        assert!(value(&canonical[0])["text"].is_array());
        let mut expected = canonical[0].clone();
        let Part::FunctionResponse {
            function_response, ..
        } = &mut expected.parts[0]
        else {
            unreachable!()
        };
        function_response.response = json!({"output":output});
        assert_eq!(encoded(&request), encoded(&[expected]));
        let projected = encoded(&request);
        project(&mut request);
        assert_eq!(encoded(&request), projected);
    }

    #[test]
    fn retains_distinct_text_and_other_content() {
        let output = json!({"value":1,"error":"business data"});
        let mut request = vec![response(
            "mcp_fixture_read",
            json!({"output":output,"text":[output.to_string(),"Extra details", "{\"value\":2}",null],"sailry_content":{"text":"Original"},"isError":true}),
        )];
        project(&mut request);
        assert_eq!(
            value(&request[0]),
            &json!({"output":output,"text":["Extra details","{\"value\":2}",null],"sailry_content":{"text":"Original"},"isError":true})
        );
    }

    #[test]
    fn preserves_other_tools_and_unstructured_results() {
        let canonical = vec![
            response(
                "plugin_fixture_read",
                json!({"output":{"value":1},"text":["{\"value\":1}"]}),
            ),
            response(
                "mcp_fixture_read",
                json!({"output":"Original text","text":["Original text"]}),
            ),
            response(
                "mcp_fixture_read",
                json!({"error":"Service quota exhausted"}),
            ),
        ];
        let mut request = canonical.clone();
        project(&mut request);
        assert_eq!(encoded(&request), encoded(&canonical));
    }
}
