//! Declared tool media becomes SDK content only in the outgoing projection.
use super::*;
use rmcp::model::{ContentBlock, ResourceContents};

pub(super) async fn project(
    db: &Connection,
    mut value: Value,
    attachments: &[Attachment],
) -> Result<CallToolResult, Fault> {
    let mut media = Vec::new();
    let envelope = if value["kind"] == "computer" {
        value.get_mut("data")
    } else {
        Some(&mut value)
    };
    if let Some(fields) = envelope.and_then(Value::as_object_mut) {
        if let Some(inline) = fields.remove("inline_data") {
            let items = inline
                .as_array()
                .ok_or_else(|| Fault::invalid("tool media is invalid"))?;
            for item in items {
                let mime = item["mime_type"]
                    .as_str()
                    .ok_or_else(|| Fault::invalid("tool media type is missing"))?;
                let bytes = if let Some(data) = item["data"].as_str() {
                    base64::engine::general_purpose::STANDARD
                        .decode(data)
                        .map_err(|_| Fault::invalid("tool media encoding is invalid"))?
                } else {
                    serde_json::from_value::<Vec<u8>>(item["data"].clone())
                        .map_err(|_| Fault::invalid("tool media encoding is invalid"))?
                };
                media.push(inline_content(
                    mime,
                    &bytes,
                    item.get("uri").and_then(Value::as_str),
                    item.get("annotations"),
                )?);
            }
        }
        if let Some(files) = fields.remove("file_data") {
            let items = files
                .as_array()
                .ok_or_else(|| Fault::invalid("tool media is invalid"))?;
            for item in items {
                media.push(ContentBlock::text(format!("Attachment reference: {item}")));
            }
        }
    }
    let mut result = CallToolResult::structured(value);
    result.content.extend(media);
    for attachment in attachments {
        result.content.push(match attachment {
            Attachment::Inline { asset } => inline_content(
                &asset.mime,
                &blobs::get(db, &asset.digest).await?,
                asset.uri.as_deref(),
                asset.annotations.as_ref(),
            )?,
        });
    }
    Ok(result)
}

fn inline_content(
    mime: &str,
    bytes: &[u8],
    uri: Option<&str>,
    annotations: Option<&Value>,
) -> Result<ContentBlock, Fault> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut content = if mime.starts_with("image/") {
        ContentBlock::image(encoded, mime)
    } else if mime.starts_with("audio/") {
        ContentBlock::audio(encoded, mime)
    } else {
        ContentBlock::resource(
            ResourceContents::blob(encoded, uri.unwrap_or("tool-output")).with_mime_type(mime),
        )
    };
    if let Some(annotations) = annotations {
        let annotations = serde_json::from_value(annotations.clone()).map_err(storage_error)?;
        match &mut content {
            ContentBlock::Image(value) => value.annotations = Some(annotations),
            ContentBlock::Audio(value) => value.annotations = Some(annotations),
            ContentBlock::Resource(value) => value.annotations = Some(annotations),
            _ => {}
        }
    }
    Ok(content)
}
