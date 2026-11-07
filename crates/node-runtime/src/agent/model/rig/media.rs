use rig_core::message::{Text, ToolResultContent};
use sailry_protocol::conversation::ModelApi;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const FIELD: &str = "__sailry_tool_media";

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Media {
    items: BTreeMap<String, Blob>,
    nested: bool,
}

#[derive(Serialize, Deserialize)]
struct Blob {
    mime: String,
    data: String,
}

impl Media {
    pub(super) fn capture(&mut self, mime: String, data: String) -> ToolResultContent {
        let marker = format!("sailry-media:{}", uuid::Uuid::new_v4());
        self.items.insert(marker.clone(), Blob { mime, data });
        ToolResultContent::Text(Text::new(marker))
    }

    pub(super) fn attach(mut self, params: &mut serde_json::Map<String, Value>, model: &str) {
        params.remove(FIELD);
        if !self.items.is_empty() {
            self.nested = model.to_lowercase().starts_with("gemini-3");
            params.insert(FIELD.into(), json!(self));
        }
    }
}

pub(super) fn restore(body: &mut Value, api: ModelApi) -> Result<(), serde_json::Error> {
    let Some(value) = body.as_object_mut().and_then(|body| body.remove(FIELD)) else {
        return Ok(());
    };
    let media: Media = serde_json::from_value(value)?;
    match api {
        ModelApi::Anthropic => anthropic(body, &media),
        ModelApi::Gemini => gemini(body, &media),
        _ => {}
    }
    Ok(())
}

fn anthropic(body: &mut Value, media: &Media) {
    for message in body["messages"].as_array_mut().into_iter().flatten() {
        for block in message["content"].as_array_mut().into_iter().flatten() {
            if block["type"] != "tool_result" {
                continue;
            }
            for part in block["content"].as_array_mut().into_iter().flatten() {
                if let Some(blob) = part["text"].as_str().and_then(|text| media.items.get(text)) {
                    *part = json!({
                        "type":"document",
                        "source":{"type":"base64","media_type":blob.mime,"data":blob.data},
                    });
                }
            }
        }
    }
}

fn gemini(body: &mut Value, media: &Media) {
    for content in body["contents"].as_array_mut().into_iter().flatten() {
        let Some(parts) = content["parts"].as_array_mut() else {
            continue;
        };
        let original = std::mem::take(parts);
        for mut part in original {
            let Some(response) = part
                .get_mut("functionResponse")
                .and_then(Value::as_object_mut)
            else {
                parts.push(part);
                continue;
            };
            let mut blobs = Vec::new();
            if let Some(values) = response.get_mut("response").and_then(Value::as_object_mut) {
                values.retain(|_, value| retain(value, media, &mut blobs));
                if values.is_empty() {
                    response.remove("response");
                }
            }
            if media.nested {
                if !blobs.is_empty() {
                    response.insert(
                        "parts".into(),
                        Value::Array(
                            blobs
                                .into_iter()
                                .map(|blob| {
                                    json!({"inlineData":{"mimeType":blob.mime,"data":blob.data}})
                                })
                                .collect(),
                        ),
                    );
                }
            } else {
                parts.extend(
                    blobs.into_iter().map(
                        |blob| json!({"inline_data":{"mime_type":blob.mime,"data":blob.data}}),
                    ),
                );
            }
            parts.push(part);
        }
    }
}

fn retain<'a>(value: &mut Value, media: &'a Media, blobs: &mut Vec<&'a Blob>) -> bool {
    match value {
        Value::String(text) => {
            let mut retained = Vec::new();
            let mut found = false;
            for line in text.split('\n') {
                if let Some(blob) = media.items.get(line) {
                    blobs.push(blob);
                    found = true;
                } else {
                    retained.push(line);
                }
            }
            if found {
                *text = retained.join("\n");
                return !text.is_empty();
            }
        }
        Value::Array(values) => {
            values.retain_mut(|value| retain(value, media, blobs));
            return !values.is_empty();
        }
        _ => {}
    }
    true
}
