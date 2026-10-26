//! Text-only consumers share ADK's streamed delta and authoritative content contract.
use adk_core::{LlmResponse, Part};

pub(super) fn append(text: &mut String, response: &LlmResponse, limit: usize) -> bool {
    if !response.partial
        && response
            .provider_metadata
            .as_ref()
            .is_some_and(|metadata| metadata["content_complete"].as_bool() == Some(true))
    {
        text.clear();
    }
    if let Some(content) = &response.content {
        for part in &content.parts {
            if let Part::Text { text: value } = part {
                if value.len() > limit.saturating_sub(text.len()) {
                    return false;
                }
                text.push_str(value);
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use adk_core::Content;
    use serde_json::json;

    fn response(text: &str, partial: bool, complete: bool) -> LlmResponse {
        LlmResponse {
            content: Some(Content::new("model").with_text(text)),
            partial,
            provider_metadata: complete.then(|| json!({"content_complete": true})),
            ..Default::default()
        }
    }

    #[test]
    fn retains_non_authoritative_chunks() {
        let mut text = String::new();
        for chunk in [response("A", true, false), response("B", false, false)] {
            assert!(append(&mut text, &chunk, 8));
        }
        assert_eq!(text, "AB");
    }

    #[test]
    fn replaces_only_authoritative_content() {
        let mut text = String::from("Draft");
        assert!(append(&mut text, &response(" delta", true, true), 32));
        assert_eq!(text, "Draft delta");
        assert!(append(&mut text, &response("Final", false, true), 8));
        assert_eq!(text, "Final");
        assert!(append(&mut text, &response("", false, true), 8));
        assert!(text.is_empty());
    }

    #[test]
    fn bounds_accumulated_bytes() {
        let mut text = String::from("1234");
        assert!(!append(&mut text, &response("56789", true, false), 8));
        assert_eq!(text, "1234");
        assert!(append(&mut text, &response("replacement", false, true), 11));
        assert_eq!(text, "replacement");
    }
}
