//! Completion requires a final answer or a resolved planning review.
use adk_core::{Event, Part};
use sailry_protocol::{ErrorCode, Fault};

pub(super) fn validate(event: &Event) -> Result<(), Fault> {
    if !event.llm_response.partial
        && event.llm_response.turn_complete
        && event
            .llm_response
            .provider_metadata
            .as_ref()
            .is_some_and(|metadata| {
                matches!(
                    metadata["openai"]["status"].as_str(),
                    Some("incomplete" | "failed" | "cancelled")
                )
            })
    {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "Model response was not completed",
        ));
    }
    Ok(())
}

pub(super) fn present(event: &Event) -> bool {
    if event.llm_response.partial || !event.is_final_response() {
        return false;
    }
    event.content().is_some_and(|content| {
        content.role == "model"
            && content.parts.iter().any(|part| match part {
                Part::Text { text } => !text.trim().is_empty(),
                Part::InlineData { data, .. } => !data.is_empty(),
                Part::FileData { file_uri, .. } => !file_uri.trim().is_empty(),
                Part::EmbeddedResource { .. } => true,
                _ => false,
            })
    })
}

pub(super) fn reviewed(event: &Event) -> bool {
    if event.llm_response.partial || !event.actions.skip_summarization {
        return false;
    }
    // The Node sets this action only after consuming a durable plan response.
    event.content().is_some_and(|content| {
        content.role == "function"
            && content.parts.iter().any(|part| {
                matches!(part, Part::FunctionResponse { function_response, .. }
                    if function_response.name == "ask_user"
                    && matches!(function_response.response["status"].as_str(), Some("answered" | "accepted")))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use adk_core::{Content, FunctionResponseData};
    use serde_json::json;

    fn event(parts: Vec<Part>) -> Event {
        let mut event = Event::new("fixture");
        event.author = "assistant".into();
        event.llm_response.content = Some(Content {
            role: "model".into(),
            parts,
        });
        event.llm_response.turn_complete = true;
        event
    }

    #[test]
    fn accepts_final_content() {
        for part in [
            Part::Text {
                text: "An answer".into(),
            },
            Part::InlineData {
                mime_type: "image/png".into(),
                data: vec![1],
                uri: None,
                annotations: None,
            },
            Part::FileData {
                mime_type: "image/png".into(),
                file_uri: "https://example.com/image.png".into(),
                annotations: None,
            },
        ] {
            assert!(present(&event(vec![part])));
        }
    }

    #[test]
    fn excludes_empty_and_internal_content() {
        for parts in [
            vec![],
            vec![Part::Text {
                text: " \n\t".into(),
            }],
            vec![Part::Thinking {
                thinking: "Inspecting the task".into(),
                signature: None,
            }],
            vec![Part::ServerToolCall {
                server_tool_call: json!({
                    "type":"message", "phase":"commentary", "content":[{"type":"output_text", "text":"Working"}]
                }),
            }],
            vec![
                Part::Text {
                    text: "Reading the file".into(),
                },
                Part::FunctionCall {
                    id: Some("call".into()),
                    name: "read_file".into(),
                    args: json!({}),
                    thought_signature: None,
                },
            ],
        ] {
            assert!(!present(&event(parts)));
        }
    }

    #[test]
    fn excludes_partial_and_continuing_content() {
        let mut answer = event(vec![Part::Text {
            text: "An answer".into(),
        }]);
        answer.llm_response.partial = true;
        assert!(!present(&answer));
        answer.llm_response.partial = false;
        answer.llm_response.provider_metadata = Some(json!({"continue_turn":true}));
        assert!(!present(&answer));
        answer.llm_response.provider_metadata = None;
        answer.llm_response.content.as_mut().unwrap().role = "tool".into();
        assert!(!present(&answer));
    }

    #[test]
    fn rejects_incomplete_terminal_status() {
        for status in ["incomplete", "failed", "cancelled"] {
            let mut answer = event(vec![Part::Text {
                text: "An unfinished answer".into(),
            }]);
            answer.llm_response.provider_metadata = Some(json!({"openai":{"status":status}}));
            assert_eq!(
                validate(&answer).unwrap_err().message,
                "Model response was not completed"
            );
            answer.llm_response.partial = true;
            assert!(validate(&answer).is_ok());
        }
    }

    #[test]
    fn accepts_resolved_reviews() {
        for status in ["answered", "accepted"] {
            let mut review = event(vec![Part::FunctionResponse {
                function_response: FunctionResponseData::new("ask_user", json!({"status":status})),
                id: Some("review".into()),
                annotations: None,
            }]);
            review.llm_response.content.as_mut().unwrap().role = "function".into();
            assert!(!reviewed(&review));
            review.actions.skip_summarization = true;
            assert!(reviewed(&review));
            review.llm_response.partial = true;
            assert!(!reviewed(&review));
        }
    }

    #[test]
    fn excludes_other_tool_results() {
        for (name, status) in [
            ("ask_user", "cancelled"),
            ("ask_user", "error"),
            ("read_file", "answered"),
        ] {
            let mut result = event(vec![Part::FunctionResponse {
                function_response: FunctionResponseData::new(name, json!({"status":status})),
                id: Some("result".into()),
                annotations: None,
            }]);
            result.llm_response.content.as_mut().unwrap().role = "function".into();
            result.actions.skip_summarization = true;
            assert!(!reviewed(&result));
        }
    }
}
