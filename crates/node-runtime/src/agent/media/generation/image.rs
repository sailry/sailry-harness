use super::*;
use base64::Engine as _;

pub(super) fn request(
    route: Option<ImageRoute>,
    model: &str,
    prompt: &str,
) -> (&'static str, Value) {
    match route {
        Some(ImageRoute::Responses) => (
            "responses",
            json!({
                "model":model,
                "input":[{"role":"user", "content":prompt}],
                "tools":[{"type":"image_generation", "output_format":"png"}],
                "tool_choice":{"type":"image_generation"},
                "store":false,
                "stream":false,
                "max_tool_calls":1,
            }),
        ),
        Some(ImageRoute::Images) => ("images/generations", json!({"model":model,"prompt":prompt})),
        None => ("videos", json!({"model":model,"prompt":prompt})),
        Some(ImageRoute::Native) => unreachable!("native generation uses ADK"),
    }
}

pub(super) fn completed(value: &Value) -> Result<Vec<u8>, Fault> {
    match value.get("status").and_then(Value::as_str) {
        Some("completed") => {}
        Some("queued" | "in_progress") => return Err(uncertain()),
        _ => return Err(unavailable("image response was not completed")),
    }
    let output = value
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| unavailable("generation returned no image"))?;
    let mut calls = output
        .iter()
        .filter(|item| item["type"] == "image_generation_call");
    let call = calls
        .next()
        .ok_or_else(|| unavailable("generation returned no image"))?;
    if calls.next().is_some() {
        return Err(unavailable("generation returned multiple images"));
    }
    if call["status"] != "completed" {
        return Err(unavailable("image generation was not completed"));
    }
    let encoded = call
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| unavailable("generation returned no image content"))?;
    decode(encoded)
}

pub(super) fn decode(encoded: &str) -> Result<Vec<u8>, Fault> {
    decode_bounded(encoded, IMAGE_BYTES)
}

fn decode_bounded(encoded: &str, limit: usize) -> Result<Vec<u8>, Fault> {
    if encoded.len() > limit.div_ceil(3) * 4 {
        return Err(unavailable("generated image exceeds the size limit"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| unavailable("invalid generated image encoding"))?;
    if bytes.len() > limit {
        return Err(unavailable("generated image exceeds the size limit"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_route_and_captured_model() {
        let (endpoint, payload) = request(
            Some(ImageRoute::Responses),
            "captured-main",
            "Draw a forest",
        );
        assert_eq!(endpoint, "responses");
        assert_eq!(
            payload,
            json!({"model":"captured-main", "input":[{"role":"user","content":"Draw a forest"}],
            "tools":[{"type":"image_generation","output_format":"png"}], "tool_choice":{"type":"image_generation"},
            "store":false,"stream":false,"max_tool_calls":1})
        );
        assert_eq!(
            request(Some(ImageRoute::Images), "companion", "Draw").0,
            "images/generations"
        );
        assert_eq!(request(None, "video", "Draw").0, "videos");
    }

    #[test]
    fn requires_completed_response_and_image() {
        let mut value = json!({"status":"completed","output":[{"type":"message"},{"type":"image_generation_call","status":"completed","result":"AQID"}]});
        assert_eq!(completed(&value).unwrap(), vec![1, 2, 3]);
        for status in ["failed", "incomplete", "cancelled", "queued", "in_progress"] {
            value["status"] = status.into();
            assert!(completed(&value).is_err());
        }
        value["status"] = "completed".into();
        value["output"][1]["status"] = "in_progress".into();
        assert!(completed(&value).is_err());
        value["output"][1]["status"] = "completed".into();
        let additional = value["output"][1].clone();
        value["output"].as_array_mut().unwrap().push(additional);
        assert!(completed(&value).is_err());
    }

    #[test]
    fn bounds_decoding_before_publication() {
        assert_eq!(decode_bounded("AQID", 3).unwrap(), vec![1, 2, 3]);
        assert!(decode_bounded("AQID", 2).is_err());
        assert!(decode_bounded("AQIDAQID", 3).is_err());
        assert!(decode_bounded("invalid!", 8).is_err());
    }
}
