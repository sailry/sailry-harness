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
                "stream":true,
                "max_tool_calls":1,
            }),
        ),
        // Account-authorized Responses endpoints accept only streamed, unstored requests.
        Some(ImageRoute::Authorized) => (
            "responses",
            json!({
                "model":model,
                "input":[{"type":"message", "role":"user",
                    "content":[{"type":"input_text", "text":prompt}]}],
                "tools":[{"type":"image_generation", "output_format":"png"}],
                "tool_choice":{"type":"image_generation"},
                "store":false,
                "stream":true,
            }),
        ),
        Some(ImageRoute::Images) => ("images/generations", json!({"model":model,"prompt":prompt})),
        None => ("videos", json!({"model":model,"prompt":prompt})),
        Some(ImageRoute::Native) => {
            unreachable!("native image generation is unavailable on this route")
        }
    }
}

pub(super) fn completed(value: &Value, secrets: &[Secret]) -> Result<Vec<u8>, Fault> {
    match value.get("status").and_then(Value::as_str) {
        Some("completed") => {}
        Some("queued" | "in_progress") => return Err(uncertain()),
        _ => {
            let reason = value
                .pointer("/incomplete_details/reason")
                .filter(|reason| reason.is_string())
                .map(|reason| json!({"code":reason}));
            return Err(unavailable(&error::describe(
                "image response was not completed",
                value
                    .get("error")
                    .filter(|error| error.is_object())
                    .or(reason.as_ref()),
                None,
                secrets,
            )));
        }
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

pub(super) fn decode_bounded(encoded: &str, limit: usize) -> Result<Vec<u8>, Fault> {
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
            "store":false,"stream":true,"max_tool_calls":1})
        );
        assert_eq!(
            request(Some(ImageRoute::Images), "companion", "Draw").0,
            "images/generations"
        );
        assert_eq!(request(None, "video", "Draw").0, "videos");
    }

    #[test]
    fn streams_authorized_requests() {
        let (endpoint, payload) = request(
            Some(ImageRoute::Authorized),
            "captured-main",
            "Draw a forest",
        );
        assert_eq!(endpoint, "responses");
        assert_eq!(payload["model"], "captured-main");
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["store"], false);
        assert_eq!(
            payload["input"],
            json!([{"type":"message","role":"user",
                "content":[{"type":"input_text","text":"Draw a forest"}]}])
        );
        assert_eq!(
            payload["tools"],
            json!([{"type":"image_generation","output_format":"png"}])
        );
        assert_eq!(payload["tool_choice"], json!({"type":"image_generation"}));
    }

    #[test]
    fn requires_completed_response_and_image() {
        let mut value = json!({"status":"completed","output":[{"type":"message"},{"type":"image_generation_call","status":"completed","result":"AQID"}]});
        assert_eq!(completed(&value, &[]).unwrap(), vec![1, 2, 3]);
        for status in ["failed", "incomplete", "cancelled", "queued", "in_progress"] {
            value["status"] = status.into();
            assert!(completed(&value, &[]).is_err());
        }
        value["status"] = "completed".into();
        value["output"][1]["status"] = "in_progress".into();
        assert!(completed(&value, &[]).is_err());
        value["output"][1]["status"] = "completed".into();
        let additional = value["output"][1].clone();
        value["output"].as_array_mut().unwrap().push(additional);
        assert!(completed(&value, &[]).is_err());
    }

    #[test]
    fn separates_pending_from_terminal_failures() {
        for status in ["queued", "in_progress"] {
            let error = completed(&json!({"status":status}), &[]).unwrap_err();
            assert_eq!(error.code, ErrorCode::OutcomeUnknown);
        }
        let secrets = [Secret::new("isolated-image-access".into())];
        let error = completed(
            &json!({"status":"failed","error":{"code":"moderation_blocked",
                "message":"Rejected for isolated-image-access"}}),
            &secrets,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(error.message.contains("moderation_blocked"));
        assert!(!error.message.contains("isolated-image-access"));
        let error = completed(
            &json!({"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"}}),
            &[],
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(error.message.contains("max_output_tokens"));
    }

    #[test]
    fn bounds_decoding_before_publication() {
        assert_eq!(decode_bounded("AQID", 3).unwrap(), vec![1, 2, 3]);
        assert!(decode_bounded("AQID", 2).is_err());
        assert!(decode_bounded("AQIDAQID", 3).is_err());
        assert!(decode_bounded("invalid!", 8).is_err());
    }
}
