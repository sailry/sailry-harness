//! Legacy backend Responses contract, reviewed against CLIProxyAPI 0f96f568.
use super::*;
use sailry_protocol::SessionId;

pub(super) fn adapt(
    body: &mut Value,
    headers: &mut HeaderMap,
    session: SessionId,
) -> Result<(), AdkError> {
    let fields = body.as_object_mut().ok_or_else(invalid)?;
    fields.insert("stream".into(), json!(true));
    if fields.get("instructions").is_none_or(Value::is_null) {
        fields.insert("instructions".into(), json!(""));
    }
    if !fields["instructions"].is_string() {
        return Err(invalid());
    }
    for item in fields
        .get_mut("input")
        .and_then(Value::as_array_mut)
        .ok_or_else(invalid)?
    {
        if item["role"] == "system" {
            item.as_object_mut()
                .ok_or_else(invalid)?
                .insert("role".into(), json!("developer"));
        }
    }
    for key in [
        "max_output_tokens",
        "max_completion_tokens",
        "max_tool_calls",
        "temperature",
        "top_p",
        "generate",
        "prompt_cache_options",
        "prompt_cache_retention",
        "safety_identifier",
        "stream_options",
        "truncation",
        "user",
    ] {
        fields.remove(key);
    }
    if fields
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| tools.len() == 1 && tools[0]["type"] == "image_generation")
    {
        fields.insert("tool_choice".into(), json!({"type":"image_generation"}));
    }
    let session = session.to_string();
    fields.insert("prompt_cache_key".into(), json!(session));
    headers.insert(
        "session-id",
        HeaderValue::from_str(&session).map_err(|_| invalid())?,
    );
    let model = fields
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    headers.insert(
        "x-codex-routing-hint",
        HeaderValue::from_str(&format!("model={model}")).map_err(|_| invalid())?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_backend_tools_and_session() {
        let session = SessionId::new();
        let mut headers = HeaderMap::new();
        let tools = json!([
            {"type":"function","name":"read_file","parameters":{}},
            {"type":"image_generation","output_format":"png"}
        ]);
        let mut body = json!({"model":"captured-model", "input":[
            {"role":"system","content":"Follow the task"},
            {"type":"function_call","name":"read_file","arguments":"{}","call_id":"call"},
            {"type":"function_call_output","call_id":"call","output":"Contents"}
        ], "tools":tools, "stream":false, "store":true, "max_output_tokens":10,
        "max_tool_calls":1, "temperature":0.5, "top_p":0.9, "previous_response_id":"old"});
        adapt(&mut body, &mut headers, session).unwrap();
        super::super::super::hosted::stateless(&mut body, true).unwrap();
        assert_eq!(body["model"], "captured-model");
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["instructions"], "");
        assert_eq!(body["input"][0]["role"], "developer");
        assert!(body["input"][1].get("namespace").is_none());
        assert_eq!(body["tools"], tools);
        assert_eq!(body["prompt_cache_key"], session.to_string());
        assert_eq!(headers["session-id"], session.to_string());
        assert_eq!(headers["x-codex-routing-hint"], "model=captured-model");
        for field in [
            "max_output_tokens",
            "max_tool_calls",
            "temperature",
            "top_p",
            "previous_response_id",
        ] {
            assert!(body.get(field).is_none());
        }
        adapt(&mut body, &mut headers, session).unwrap();
        super::super::super::hosted::stateless(&mut body, true).unwrap();
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    }

    #[test]
    fn preserves_native_search_declarations() {
        let tools = json!([
            {"type":"web_search","search_context_size":"medium"},
            {"type":"function","name":"read_file","parameters":{}}
        ]);
        let mut body = json!({"model":"catalog-next", "input":[], "tools":tools});
        let mut headers = HeaderMap::new();
        adapt(&mut body, &mut headers, SessionId::new()).unwrap();
        assert_eq!(body["tools"], tools);
        assert_eq!(body["model"], "catalog-next");
        assert_eq!(headers["x-codex-routing-hint"], "model=catalog-next");
    }

    #[test]
    fn rejects_invalid_body_without_changing_contracts() {
        for mut body in [
            json!({"model":"captured", "input":"prompt"}),
            json!({"model":"captured", "input":[], "instructions":false}),
            json!({"model":"captured", "input":[], "include":false}),
        ] {
            let result = adapt(&mut body, &mut HeaderMap::new(), SessionId::new())
                .and_then(|()| super::super::super::hosted::stateless(&mut body, true));
            assert!(result.is_err());
        }
    }
}
