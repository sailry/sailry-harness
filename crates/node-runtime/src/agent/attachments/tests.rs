use super::*;
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
    assert!(
        matches!(inline(&metadata, bytes.clone(), true, None), Some(Part::InlineData { mime_type, data, .. }) if mime_type == "image/png" && data == bytes)
    );
    assert!(inline(&metadata, bytes, false, None).is_none());
    assert!(
        inline(
            &attachment("image/png"),
            b"not an image".to_vec(),
            true,
            Some(ModelApi::Gemini)
        )
        .is_none()
    );
}

#[test]
fn detects_pdf_content() {
    let metadata = attachment("text/plain");
    let bytes = b"%PDF-1.7".to_vec();
    assert!(
        matches!(inline(&metadata, bytes.clone(), true, Some(ModelApi::Responses)), Some(Part::InlineData { mime_type, data, .. }) if mime_type == "application/pdf" && data == bytes)
    );
    assert!(inline(&metadata, bytes.clone(), true, None).is_none());
    assert!(inline(&metadata, bytes, false, Some(ModelApi::Responses)).is_none());
    assert!(
        inline(
            &attachment("application/pdf"),
            b"not a PDF".to_vec(),
            true,
            Some(ModelApi::Responses)
        )
        .is_none()
    );
}

#[test]
fn detects_audio_and_video_content() {
    let metadata = attachment("text/plain");
    let fixtures: &[(&[u8], &str, bool)] = &[
        (
            include_bytes!("../../../../../tests/fixtures/audio.wav"),
            "audio/wav",
            false,
        ),
        (
            include_bytes!("../../../../../tests/fixtures/audio.mp4"),
            "audio/m4a",
            false,
        ),
        (
            include_bytes!("../../../../../tests/fixtures/audio.mp3"),
            "audio/mpeg",
            false,
        ),
        (
            include_bytes!("../../../../../tests/fixtures/video.mp4"),
            "video/mp4",
            true,
        ),
        (
            include_bytes!("../../../../../tests/fixtures/video.webm"),
            "video/webm",
            true,
        ),
    ];
    for &(bytes, expected, visual) in fixtures {
        let native = inline(&metadata, bytes.to_vec(), visual, Some(ModelApi::Gemini));
        assert!(
            matches!(native, Some(Part::InlineData { mime_type, data, .. }) if mime_type == expected && data == bytes),
            "{expected}"
        );
        for api in [
            None,
            Some(ModelApi::ChatCompletions),
            Some(ModelApi::Responses),
            Some(ModelApi::Anthropic),
        ] {
            assert!(
                inline(&metadata, bytes.to_vec(), true, api).is_none(),
                "{expected}: {api:?}"
            );
        }
        if visual {
            assert!(inline(&metadata, bytes.to_vec(), false, Some(ModelApi::Gemini)).is_none());
        }
    }
    for media in ["audio/wav", "video/mp4"] {
        assert!(
            inline(
                &attachment(media),
                b"not media".to_vec(),
                true,
                Some(ModelApi::Gemini)
            )
            .is_none()
        );
    }
}

#[test]
fn preserves_text_and_defers_attachments() {
    let attachment = attachment("application/octet-stream");
    assert!(
        matches!(inline(&attachment, "输入 🙂".as_bytes().to_vec(), false, None), Some(Part::Text { text }) if text.ends_with("输入 🙂"))
    );
    assert!(
        inline(
            &attachment,
            vec![b'x'; 1024 * 1024 + 1],
            true,
            Some(ModelApi::Gemini)
        )
        .is_none()
    );
    for bytes in [
        b"binary\0content".to_vec(),
        vec![255, 0, 1],
        b"Rar!\x1a\x07\0".to_vec(),
    ] {
        assert!(inline(&attachment, bytes, true, Some(ModelApi::Gemini)).is_none());
    }
    let Part::Text { text } = file(&attachment, false) else {
        panic!("file metadata expected")
    };
    assert!(text.contains(&attachment.id.to_string()));
    assert!(text.contains("not included"));
    assert!(text.contains("an attachment-capable tool is required"));
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
fn preserves_local_errors() {
    let fault = invalid("attachment is not UTF-8 text");
    assert_eq!(
        model::error(
            adk_core::AdkError::model("attachment input failed").with_source(fault.clone())
        ),
        fault
    );
    let unsafe_body = "untrusted upstream response body";
    assert!(
        !model::error(adk_core::AdkError::model(unsafe_body))
            .message
            .contains(unsafe_body)
    );
}

#[test]
fn filters_images_for_text_models() {
    let mut content = Content::new("model").with_text("Generated image");
    content.parts.push(Part::InlineData {
        mime_type: "image/png".into(),
        data: vec![1, 2, 3],
        uri: None,
        annotations: None,
    });
    content.parts.push(Part::FunctionResponse {
        id: Some("generate".into()),
        annotations: None,
        function_response: adk_core::FunctionResponseData::with_inline_data(
            "generate",
            serde_json::json!({"path":"image.png"}),
            ["image/png", "audio/wav"]
                .map(|mime_type| adk_core::InlineDataPart {
                    mime_type: mime_type.into(),
                    data: vec![1, 2, 3],
                    uri: None,
                    annotations: None,
                })
                .to_vec(),
        ),
    });
    let canonical = vec![content];
    let mut request = canonical.clone();
    filter_images(&mut request);
    assert!(matches!(&canonical[0].parts[1], Part::InlineData { .. }));
    assert_eq!(request[0].parts[0].text(), Some("Generated image"));
    assert!(
        request[0].parts[1]
            .text()
            .unwrap()
            .contains("does not support images")
    );
    let Part::FunctionResponse {
        function_response, ..
    } = &request[0].parts[2]
    else {
        panic!("tool result expected")
    };
    assert_eq!(
        function_response.response,
        serde_json::json!({"path":"image.png"})
    );
    assert_eq!(function_response.inline_data.len(), 1);
    assert_eq!(function_response.inline_data[0].mime_type, "audio/wav");
    let Part::FunctionResponse {
        function_response, ..
    } = &canonical[0].parts[2]
    else {
        panic!("canonical tool result expected")
    };
    assert_eq!(function_response.inline_data.len(), 2);
}
