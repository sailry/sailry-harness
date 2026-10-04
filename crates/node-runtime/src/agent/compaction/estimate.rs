//! Approximate pending text only; provider observations remain authoritative.
use adk_core::{EmbeddedResource, Event, Part};
use serde_json::Value;

pub(super) fn tokens(events: &[Event]) -> u64 {
    events
        .iter()
        .filter_map(Event::content)
        .map(|content| 4 + content.parts.iter().map(part).sum::<u64>())
        .sum()
}

fn part(part: &Part) -> u64 {
    match part {
        Part::Text { text: value }
        | Part::Thinking {
            thinking: value, ..
        } => text(value),
        Part::FunctionCall { name, args, .. } => text(name) + json(args),
        Part::FunctionResponse {
            function_response, ..
        } => text(&function_response.name) + json(&function_response.response),
        Part::ServerToolCall { server_tool_call } => json(server_tool_call),
        Part::ServerToolResponse {
            server_tool_response,
        } => json(server_tool_response),
        Part::EmbeddedResource {
            resource: EmbeddedResource::Text(resource),
        } => text(&resource.text),
        // Binary length is unrelated to model token cost. The next provider
        // observation accounts for media; the early threshold leaves headroom.
        Part::InlineData { .. }
        | Part::FileData { .. }
        | Part::EmbeddedResource {
            resource: EmbeddedResource::Blob(_),
        } => 0,
    }
}

fn text(value: &str) -> u64 {
    // A lightweight UTF-8 text estimate, not a tokenizer or billed usage.
    (value.len() as u64).div_ceil(4)
}

fn json(value: &Value) -> u64 {
    text(&value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use adk_core::{BlobResourceContents, Content, FunctionResponseData, InlineDataPart};
    use serde_json::json;

    fn event(parts: Vec<Part>) -> Event {
        let mut event = Event::new("pending-fixture");
        event.set_content(Content {
            role: "user".into(),
            parts,
        });
        event
    }

    #[test]
    fn accounts_for_tool_text() {
        let result = |size| {
            event(vec![Part::FunctionResponse {
                id: Some("read-1".into()),
                function_response: FunctionResponseData::new(
                    "read_file",
                    json!({
                        "text": "a".repeat(size), "path": "source.txt"
                    }),
                ),
                annotations: None,
            }])
        };
        assert!(tokens(&[result(48 * 1024)]) + 110 < 12_800);
        assert!(tokens(&[result(64 * 1024)]) + 110 > 12_800);
    }

    #[test]
    fn counts_user_and_embedded_text() {
        let user = event(vec![Part::Text {
            text: "新任务 🙂".repeat(100),
        }]);
        let resource = event(vec![Part::EmbeddedResource {
            resource: EmbeddedResource::Text(adk_core::TextResourceContents::new(
                "file:///source.txt",
                Some("text/plain".into()),
                "More evidence".repeat(100),
            )),
        }]);
        let separately =
            tokens(std::slice::from_ref(&user)) + tokens(std::slice::from_ref(&resource));
        assert!(separately > 500);
        assert_eq!(tokens(&[user, resource]), separately);
        assert_eq!(tokens(&[Event::new("empty")]), 0);
    }

    #[test]
    fn excludes_media_bytes() {
        let content = |size| {
            let mut response = FunctionResponseData::new("observe", json!({"status":"observed"}));
            response.inline_data.push(InlineDataPart {
                mime_type: "image/png".into(),
                data: vec![255; size],
                uri: None,
                annotations: None,
            });
            event(vec![
                Part::InlineData {
                    mime_type: "image/png".into(),
                    data: vec![255; size],
                    uri: None,
                    annotations: None,
                },
                Part::EmbeddedResource {
                    resource: EmbeddedResource::Blob(
                        BlobResourceContents::new(
                            "file:///screen.png",
                            Some("image/png".into()),
                            vec![255; size],
                        )
                        .unwrap(),
                    ),
                },
                Part::FunctionResponse {
                    id: Some("observe-1".into()),
                    function_response: response,
                    annotations: None,
                },
            ])
        };
        let small = tokens(&[content(16)]);
        assert!(small < 100);
        assert_eq!(tokens(&[content(2 * 1024 * 1024)]), small);
    }
}
