use super::*;

#[test]
fn uses_stateless_streaming() {
    let mut body = json!({"model":"selected-fixture","stream":true,"store":true,"instructions":"Keep context","input":[{"role":"user","content":"Continue"}],"include":[],"max_output_tokens":20,"temperature":0.5,"top_p":0.9,"background":true,"metadata":{},"parallel_tool_calls":true,"service_tier":"auto","text":{},"user":"fixture"});
    chatgpt(&mut body).unwrap();
    assert_eq!(body["model"], "selected-fixture");
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert_eq!(body["instructions"], "Keep context");
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    for key in [
        "max_output_tokens",
        "temperature",
        "top_p",
        "background",
        "metadata",
        "parallel_tool_calls",
        "service_tier",
        "text",
        "user",
    ] {
        assert!(body.get(key).is_none(), "unexpected field: {key}");
    }
    chatgpt(&mut body).unwrap();
    assert_eq!(body["include"].as_array().unwrap().len(), 1);
}

#[test]
fn preserves_native_tools_and_history() {
    let input = json!([{"role":"system","content":"Follow the task"},{"type":"function_call","name":"read_file","call_id":"call_1","arguments":"{}"},{"type":"function_call_output","call_id":"call_1","output":"File contents"}]);
    let tools = json!([{"type":"function","name":"read_file","parameters":{"type":"object","properties":{}}}]);
    let mut body = json!({"stream":true,"input":input,"tools":tools});
    chatgpt(&mut body).unwrap();
    assert_eq!(body["input"], input);
    assert_eq!(body["tools"], tools);
    assert_eq!(body["instructions"], "");
}

#[test]
fn rejects_invalid_stream_and_include() {
    for mut body in [
        json!({"stream":false}),
        json!({"stream":true,"include":false}),
    ] {
        assert!(chatgpt(&mut body).is_err());
    }
}
