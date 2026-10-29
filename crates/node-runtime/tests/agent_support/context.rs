use super::*;

const CONTEXT: &str = "<turn-context>\nSession: fixture\n</turn-context>";

#[test]
fn preserves_trailing_newlines() {
    let task = "Original task\n\n";
    let expected = json!({"messages":[{"role":"user", "content":task}]});
    let body = json!({"messages":[{
        "role":"user", "content":format!("{task}\n{CONTEXT}")
    }]});
    let captured = body.clone();
    assert_eq!(task_request(&body), expected);
    assert_eq!(body, captured);
}

#[test]
fn removes_terminal_parts() {
    for (field, key, content, context) in [
        (
            "messages",
            "content",
            json!([{"type":"tool_result", "tool_use_id":"call", "content":"Retained output"}]),
            json!({"type":"text", "text":CONTEXT}),
        ),
        (
            "input",
            "content",
            json!([{"type":"input_text", "text":"Original task\n\n"}]),
            json!({"type":"input_text", "text":CONTEXT}),
        ),
        (
            "contents",
            "parts",
            json!([{"text":"Original task\n\n"}]),
            json!({"text":CONTEXT}),
        ),
    ] {
        let mut message = json!({"role":"user"});
        message[key] = content;
        let mut expected = json!({});
        expected[field] = json!([message]);

        let mut merged = expected.clone();
        merged[field][0][key]
            .as_array_mut()
            .unwrap()
            .push(context.clone());
        let captured = merged.clone();
        assert_eq!(task_request(&merged), expected);
        assert_eq!(merged, captured);

        let mut separate = expected.clone();
        let mut tail = json!({"role":"user"});
        tail[key] = json!([context]);
        separate[field].as_array_mut().unwrap().push(tail);
        let captured = separate.clone();
        assert_eq!(task_request(&separate), expected);
        assert_eq!(separate, captured);
    }
}
