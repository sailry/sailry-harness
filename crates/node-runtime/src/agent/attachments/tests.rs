use super::*;
use goose_provider_types::conversation::message::Message as GdkMessage;
use rmcp::model::{CallToolResult, ContentBlock};
use sailry_protocol::{WorktreeId, attachment::Spec};

fn attachment(media: &str) -> Attachment {
    Attachment {
        id: AttachmentId::new(),
        spec: Spec {
            worktree: WorktreeId::new(),
            name: "input".into(),
            media_type: media.into(),
            size: 0,
            revision: String::new(),
        },
    }
}

#[test]
fn detects_image_content() {
    let metadata = attachment("text/plain");
    let bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    let Some(MessageContent::Image(image)) =
        inline(&metadata, bytes.clone(), true, ModelApi::Responses)
    else {
        panic!("image expected")
    };
    assert_eq!(image.mime_type, "image/png");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(&image.data)
            .unwrap(),
        bytes
    );
    assert!(inline(&metadata, bytes, false, ModelApi::Responses).is_none());
    assert!(!matches!(
        inline(
            &attachment("image/png"),
            b"not an image".to_vec(),
            true,
            ModelApi::Gemini
        ),
        Some(MessageContent::Image(_))
    ));
}

#[test]
fn separates_documents_from_vision() {
    let metadata = attachment("text/plain");
    let bytes = b"%PDF-1.7".to_vec();
    for api in [
        ModelApi::Responses,
        ModelApi::ChatCompletions,
        ModelApi::Anthropic,
        ModelApi::Gemini,
        ModelApi::AzureOpenAi,
    ] {
        for vision in [false, true] {
            let Some(MessageContent::Document(document)) =
                inline(&metadata, bytes.clone(), vision, api)
            else {
                panic!("document expected")
            };
            assert_eq!(document.mime_type, "application/pdf");
            assert_eq!(document.name.as_deref(), Some("input"));
            assert_eq!(
                base64::engine::general_purpose::STANDARD
                    .decode(document.data)
                    .unwrap(),
                bytes
            );
        }
    }
    assert!(!matches!(
        inline(
            &attachment("application/pdf"),
            b"not a PDF".to_vec(),
            true,
            ModelApi::Responses
        ),
        Some(MessageContent::Document(_))
    ));
}

#[test]
fn retains_unsupported_media_references() {
    for bytes in [
        include_bytes!("../../../../../tests/fixtures/audio.wav").as_slice(),
        include_bytes!("../../../../../tests/fixtures/audio.mp4").as_slice(),
        include_bytes!("../../../../../tests/fixtures/audio.mp3").as_slice(),
        include_bytes!("../../../../../tests/fixtures/video.mp4").as_slice(),
        include_bytes!("../../../../../tests/fixtures/video.webm").as_slice(),
    ] {
        assert!(
            inline(
                &attachment("text/plain"),
                bytes.to_vec(),
                true,
                ModelApi::Gemini
            )
            .is_none()
        );
    }
    let metadata = attachment("audio/wav");
    let text = file(&metadata, Some(READ_NOTE));
    assert!(text.contains(&metadata.id.to_string()));
    assert!(text.contains("Contents not included"));
}

#[test]
fn preserves_text_and_defers_attachments() {
    let metadata = attachment("application/octet-stream");
    assert!(
        matches!(inline(&metadata, "输入 🙂".as_bytes().to_vec(), false, ModelApi::Responses), Some(MessageContent::Text(text)) if text.text.ends_with("输入 🙂"))
    );
    assert!(
        inline(
            &metadata,
            vec![b'x'; 1024 * 1024 + 1],
            true,
            ModelApi::Gemini
        )
        .is_none()
    );
    for bytes in [
        b"binary\0content".to_vec(),
        vec![255, 0, 1],
        b"Rar!\x1a\x07\0".to_vec(),
    ] {
        assert!(inline(&metadata, bytes, true, ModelApi::Gemini).is_none());
    }
    let text = file(&metadata, Some(READ_NOTE));
    assert!(text.contains(&metadata.id.to_string()));
    assert!(text.contains("read_office"));
    assert!(text.contains("\"kind\":\"attachment\""));
    assert!(!text.contains(PREFIX));
}

#[test]
fn accepts_only_owned_references() {
    let attachment = AttachmentId::new();
    assert_eq!(id(&format!("{PREFIX}{attachment}")).unwrap(), attachment);
    for value in [
        String::new(),
        format!("file:///{attachment}"),
        format!("{PREFIX}{attachment}/../other"),
        format!("{PREFIX}{}", attachment.to_string().to_uppercase()),
    ] {
        assert!(id(&value).is_err());
    }
}

#[test]
fn filters_images_for_text_models() {
    let message = GdkMessage::assistant()
        .with_text("Generated image")
        .with_image("AQID", "image/png");
    let result = CallToolResult::success(vec![
        ContentBlock::text("original result"),
        ContentBlock::image("AQID", "image/png"),
        ContentBlock::audio("AQID", "audio/wav"),
    ]);
    let canonical = Conversation::new_unvalidated(vec![
        message,
        GdkMessage::user().with_tool_response("generate", Ok(result)),
    ]);
    let mut request = canonical.clone();
    filter_images(&mut request);
    assert!(matches!(
        &canonical.messages()[0].content[1],
        MessageContent::Image(_)
    ));
    assert!(
        matches!(&request.messages()[0].content[0],MessageContent::Text(text) if text.text=="Generated image")
    );
    assert!(
        matches!(&request.messages()[0].content[1],MessageContent::Text(text) if text.text.contains("does not support images"))
    );
    let MessageContent::ToolResponse(response) = &request.messages()[1].content[0] else {
        unreachable!()
    };
    let result = response.tool_result.as_ref().unwrap();
    assert_eq!(result.content[0].as_text().unwrap().text, "original result");
    assert!(
        result.content[1]
            .as_text()
            .unwrap()
            .text
            .contains("does not support images")
    );
    assert!(matches!(&result.content[2], ContentBlock::Audio(_)));
    let MessageContent::ToolResponse(response) = &canonical.messages()[1].content[0] else {
        unreachable!()
    };
    assert!(matches!(
        &response.tool_result.as_ref().unwrap().content[1],
        ContentBlock::Image(_)
    ));
}
