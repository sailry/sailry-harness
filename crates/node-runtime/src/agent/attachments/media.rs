use adk_core::Part;
use file_format::FileFormat;
use sailry_protocol::{attachment::Attachment, conversation::ModelApi};

pub(super) fn inline(
    attachment: &Attachment,
    bytes: Vec<u8>,
    vision: bool,
    api: Option<ModelApi>,
) -> Option<Part> {
    let format = FileFormat::from_bytes(&bytes);
    if let Some(media) = native(&format, vision, api) {
        return Some(Part::InlineData {
            mime_type: media.into(),
            data: bytes,
            uri: None,
            annotations: None,
        });
    }
    // Recognized binary formats must not become text through a misleading MIME hint.
    if !(matches!(format, FileFormat::ArbitraryBinaryData | FileFormat::Empty)
        || (format.media_type() != "application/octet-stream" && text_media(format.media_type())))
        || bytes.len() > 1024 * 1024
        || !text_media(&attachment.spec.media_type)
    {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?;
    if text.contains('\0') {
        return None;
    }
    Some(Part::Text {
        text: format!("Attachment: {}\n{text}", attachment.spec.name),
    })
}

fn native(format: &FileFormat, vision: bool, api: Option<ModelApi>) -> Option<&str> {
    use FileFormat::*;
    match format {
        PortableNetworkGraphics
        | JointPhotographicExpertsGroup
        | GraphicsInterchangeFormat
        | Webp
            if vision =>
        {
            Some(format.media_type())
        }
        PortableDocumentFormat
            if vision
                && matches!(
                    api,
                    Some(
                        ModelApi::ChatCompletions
                            | ModelApi::Responses
                            | ModelApi::Anthropic
                            | ModelApi::Gemini
                            | ModelApi::AzureOpenAi
                            | ModelApi::Vertex
                            | ModelApi::Bedrock
                    )
                ) =>
        {
            Some("application/pdf")
        }
        _ if matches!(api, Some(ModelApi::Gemini | ModelApi::Vertex)) => match format {
            WaveformAudio => Some("audio/wav"),
            AudioInterchangeFileFormat => Some("audio/aiff"),
            Mpeg12AudioLayer3 | AdvancedAudioCoding | FreeLosslessAudioCodec => {
                Some(format.media_type())
            }
            AppleItunesAudio | Mpeg4Part14Audio => Some("audio/m4a"),
            OggFlac | OggOpus | OggSpeex | OggVorbis => Some("audio/ogg"),
            Mpeg4Part14Video | Mpeg12Video | AppleQuicktime | FlashVideo | Webm if vision => {
                Some(format.media_type())
            }
            AudioVideoInterleave if vision => Some("video/avi"),
            _ => None,
        },
        _ => None,
    }
}

fn text_media(media: &str) -> bool {
    media.starts_with("text/")
        || matches!(
            media,
            "application/json"
                | "application/xml"
                | "application/yaml"
                | "application/javascript"
                | "application/octet-stream"
        )
}
