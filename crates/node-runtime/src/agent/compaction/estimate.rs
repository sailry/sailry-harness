//! Approximate pending text only; provider observations remain authoritative.
use adk_core::{EmbeddedResource, Event, LlmRequest, Part};
use serde_json::Value;

/// Estimate the complete neutral envelope, including the summary prompt and JSON framing.
/// This is not the provider's tokenizer or an exact context measurement.
pub(in crate::agent) fn request(request: &LlmRequest) -> adk_core::Result<u64> {
    Ok(envelope(request)?
        + request
            .contents
            .iter()
            .map(|content| 4 + content.parts.iter().map(part).sum::<u64>())
            .sum::<u64>())
}

pub(in crate::agent) fn envelope(request: &LlmRequest) -> adk_core::Result<u64> {
    let mut config = request.config.clone();
    if let Some(config) = &mut config {
        config
            .extensions
            .remove(super::super::model::history::PREPARED);
    }
    let envelope = LlmRequest {
        model: request.model.clone(),
        contents: Vec::new(),
        config,
        tools: Default::default(),
        previous_response_id: request.previous_response_id.clone(),
    };
    // LlmRequest deliberately skips tools during serialization.
    let framing = serde_json::to_string(&(&envelope, &request.tools))
        .map_err(|_| adk_core::AdkError::model("context request could not be encoded"))?;
    Ok(text(&framing))
}

pub(crate) fn tokens(events: &[Event]) -> u64 {
    events
        .iter()
        .filter_map(Event::content)
        .map(|content| 4 + content.parts.iter().map(part).sum::<u64>())
        .sum()
}

pub(in crate::agent) fn part(part: &Part) -> u64 {
    match part {
        Part::Text { text: value }
        | Part::Thinking {
            thinking: value, ..
        } => text(value),
        Part::FunctionCall { name, args, .. } => text(name) + json(args),
        Part::FunctionResponse {
            function_response, ..
        } => response(function_response, json(&function_response.response)),
        Part::ProviderData { .. } => match part.provider_part() {
            Part::ProviderData { .. } => 0,
            inner => self::part(inner),
        },
        Part::ServerToolCall { server_tool_call } => json(server_tool_call),
        Part::ServerToolResponse {
            server_tool_response,
        } => json(server_tool_response),
        Part::EmbeddedResource {
            resource: EmbeddedResource::Text(resource),
        } => text(&resource.text),
        Part::InlineData {
            mime_type, data, ..
        } => media(mime_type, data),
        Part::FileData { mime_type, .. } => media(mime_type, &[]),
        Part::EmbeddedResource {
            resource: EmbeddedResource::Blob(blob),
        } => media(blob.mime_type.as_deref().unwrap_or(""), &blob.data),
    }
}

pub(in crate::agent) fn response(
    response: &adk_core::FunctionResponseData,
    value_tokens: u64,
) -> u64 {
    text(&response.name)
        + value_tokens
        + response
            .inline_data
            .iter()
            .map(|part| media(&part.mime_type, &part.data))
            .sum::<u64>()
        + response
            .file_data
            .iter()
            .map(|part| media(&part.mime_type, &[]))
            .sum::<u64>()
}

pub(in crate::agent) fn text(value: &str) -> u64 {
    let mut count = 0u64;
    let mut ascii = 0u64;
    for character in value.chars() {
        if character.is_ascii() {
            ascii += 1;
            continue;
        }
        count += ascii.div_ceil(4);
        ascii = 0;
        let code = u32::from(character);
        let cjk = matches!(code, 0x2e80..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff | 0xff00..=0xffef | 0x20000..=0x323af);
        count += if cjk {
            2
        } else {
            (character.len_utf8() as u64).div_ceil(2)
        };
    }
    count + ascii.div_ceil(4)
}

fn media(mime: &str, data: &[u8]) -> u64 {
    if !mime.starts_with("image/") {
        return 0;
    }
    let dimensions = image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok());
    // A dimension-based tile envelope, not a provider tokenizer. Unknown
    // external images retain a nonzero allowance until usage is reported.
    dimensions
        .filter(|(width, height)| *width > 0 && *height > 0)
        .map_or(1024, |(width, height)| {
            85u64.saturating_add(
                170u64
                    .saturating_mul(u64::from(width).div_ceil(512))
                    .saturating_mul(u64::from(height).div_ceil(512)),
            )
        })
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
    fn ignores_compressed_media_size() {
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
        assert!(small >= 3 * 1024);
        assert_eq!(tokens(&[content(2 * 1024 * 1024)]), small);
    }

    #[test]
    fn accounts_for_cjk_and_dimensions() {
        assert_eq!(text("abcd"), 1);
        assert_eq!(text("中文测试"), 8);
        assert!(text("中文测试") > text("abcd"));
        let image = |width| {
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::new_rgba8(width, 16)
                .write_to(&mut bytes, image::ImageFormat::Png)
                .unwrap();
            bytes.into_inner()
        };
        let small = media("image/png", &image(16));
        let wider = media("image/png", &image(513));
        assert_eq!(small, 255);
        assert_eq!(wider, 425);
        assert_eq!(media("image/png", &[]), 1024);
        assert_eq!(media("audio/wav", &[]), 0);
    }
}
