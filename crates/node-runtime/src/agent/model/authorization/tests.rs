use super::*;

#[test]
fn uses_stateless_streaming() {
    let mut body = json!({
        "model": "selected-model",
        "stream": true,
        "store": true,
        "instructions": "Keep the full context",
        "input": [
            {"type":"message","role":"system","content":"Follow the task"},
            {"role":"user","content":"Continue"},
            {"type":"function_call","name":"read_file","call_id":"call_1","arguments":"{}"},
            {"type":"function_call_output","call_id":"call_1","output":"File contents"}
        ],
        "include": ["web_search_call.action.sources"],
        "parallel_tool_calls": true,
        "service_tier": "auto",
        "text": {"format":{"type":"text"}},
        "background": true,
        "conversation": "conversation_1",
        "max_output_tokens": 20,
        "max_tool_calls": 3,
        "metadata": {"key":"value"},
        "moderation": {},
        "multi_agent": {},
        "prompt": {},
        "prompt_cache_retention": "24h",
        "safety_identifier": "user_1",
        "temperature": 0.5,
        "top_logprobs": 2,
        "top_p": 0.9,
        "truncation": "auto",
        "user": "user_1",
        "previous_response_id": "response_1"
    });
    chatgpt(&mut body).unwrap();
    assert_eq!(body["model"], "selected-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert_eq!(body["instructions"], "Keep the full context");
    assert_eq!(body["input"][0]["role"], "developer");
    assert_eq!(body["input"][2]["name"], "read_file");
    assert_eq!(body["input"][2]["namespace"], "sailry");
    assert_eq!(body["input"][3]["output"], "File contents");
    assert_eq!(
        body["include"],
        json!([
            "web_search_call.action.sources",
            "reasoning.encrypted_content"
        ])
    );
    assert_eq!(body["parallel_tool_calls"], true);
    assert_eq!(body["service_tier"], "auto");
    assert_eq!(body["text"], json!({"format":{"type":"text"}}));
    for key in [
        "background",
        "conversation",
        "max_output_tokens",
        "max_tool_calls",
        "metadata",
        "moderation",
        "multi_agent",
        "prompt",
        "prompt_cache_retention",
        "safety_identifier",
        "temperature",
        "top_logprobs",
        "top_p",
        "truncation",
        "user",
        "previous_response_id",
    ] {
        assert!(body.get(key).is_none(), "unexpected field: {key}");
    }
    chatgpt(&mut body).unwrap();
    assert_eq!(body["include"].as_array().unwrap().len(), 2);
}

#[test]
fn groups_local_tools() {
    let function = json!({
        "type":"function","name":"read_file","description":"Read a file",
        "parameters":{"type":"object","properties":{"path":{"type":"string"}}},
        "strict":false
    });
    let custom = json!({"type":"custom","name":"patch","description":"Apply a patch"});
    let web = json!({"type":"web_search","search_context_size":"medium"});
    let existing = json!({
        "type":"namespace","name":"external","description":"External tools",
        "tools":[{"type":"function","name":"lookup","parameters":{}}]
    });
    let mut body = json!({
        "stream":true,"input":[
            {"type":"custom_tool_call","name":"patch","call_id":"call_2","input":"patch"},
            {"type":"function_call","name":"lookup","namespace":"external","call_id":"call_3","arguments":"{}"}
        ],
        "tools":[function,custom,web,existing]
    });
    chatgpt(&mut body).unwrap();
    assert_eq!(body["tools"][0], web);
    assert_eq!(body["tools"][1], existing);
    assert_eq!(body["tools"][2]["type"], "namespace");
    assert_eq!(body["tools"][2]["name"], "sailry");
    assert_eq!(body["tools"][2]["tools"], json!([function, custom]));
    assert_eq!(body["input"][0]["namespace"], "sailry");
    assert_eq!(body["input"][1]["namespace"], "external");
    chatgpt(&mut body).unwrap();
    assert_eq!(body["tools"].as_array().unwrap().len(), 3);
}

#[test]
fn rejects_unsupported_requests() {
    for mut body in [
        json!({"stream":false,"input":[]}),
        json!({"stream":true,"input":"prompt"}),
        json!({"stream":true,"input":[],"include":false}),
    ] {
        assert!(chatgpt(&mut body).is_err());
    }
    for tool in [
        "image_generation",
        "file_search",
        "code_interpreter",
        "computer_use_preview",
        "mcp",
        "tool_search",
        "programmatic_tool_calling",
    ] {
        let mut body = json!({"stream":true,"input":[],"tools":[{"type":tool}]});
        assert!(chatgpt(&mut body).is_err(), "unsupported tool: {tool}");
    }
}
