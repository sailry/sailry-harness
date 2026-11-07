use serde_json::{Value, json};

pub(super) const ANSWER: &str = "Read complete";

pub(super) fn responses(tool: bool) -> String {
    let output = if tool {
        json!([{
            "type":"function_call","id":"item","status":"completed",
            "call_id":"stable-call","name":"read","arguments":"{\"path\":\"file\"}"
        }])
    } else {
        json!([{
            "type":"message","id":"message","role":"assistant","status":"completed",
            "content":[{"type":"output_text","text":ANSWER,"annotations":[]}]
        }])
    };
    sse([
        json!({
            "type":"response.created","sequence_number":0,
            "response":{"id":"response","object":"response","created_at":1,
                "model":"fixture","status":"in_progress","output":[]}
        }),
        json!({
            "type":"response.completed","sequence_number":1,
            "response":{"id":"response","object":"response","created_at":1,
                "model":"fixture","status":"completed","output":output,
                "usage":{"input_tokens":12,"output_tokens":3,"total_tokens":15,
                    "input_tokens_details":{"cached_tokens":8},
                    "output_tokens_details":{"reasoning_tokens":2}}}
        }),
    ])
}

pub(super) fn chat() -> String {
    sse([
        json!({
            "id":"chat","object":"chat.completion.chunk","created":1,"model":"fixture",
            "choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"Consider"},
                "finish_reason":null}]
        }),
        json!({
            "id":"chat","object":"chat.completion.chunk","created":1,"model":"fixture",
            "choices":[{"index":0,"delta":{"content":ANSWER},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":12,"completion_tokens":3,"total_tokens":15,
                "prompt_tokens_details":{"cached_tokens":8},
                "completion_tokens_details":{"reasoning_tokens":2}}
        }),
    ]) + "data: [DONE]\n\n"
}

pub(super) fn anthropic(tool: bool) -> String {
    let mut events = vec![
        json!({
            "type":"message_start",
            "message":{"id":"message","type":"message","role":"assistant",
                "model":"claude-opus-4-8","content":[],"stop_reason":null,
                "stop_sequence":null,"usage":{"input_tokens":12,"output_tokens":0,
                    "cache_read_input_tokens":8,"cache_creation_input_tokens":4}}
        }),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"thinking_delta","thinking":"Consider"}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"signature_delta","signature":"signed-thinking"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,
            "content_block":{"type":"redacted_thinking","data":"redacted-data"}}),
        json!({"type":"content_block_stop","index":1}),
    ];
    if tool {
        events.extend([
            json!({"type":"content_block_start","index":2,
                "content_block":{"type":"tool_use","id":"stable-call","name":"read","input":{}}}),
            json!({"type":"content_block_delta","index":2,
                "delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}),
            json!({"type":"content_block_delta","index":2,
                "delta":{"type":"input_json_delta","partial_json":"\"file\"}"}}),
        ]);
    } else {
        events.extend([
            json!({"type":"content_block_start","index":2,
                "content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":2,
                "delta":{"type":"text_delta","text":ANSWER}}),
        ]);
    }
    events.extend([
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"message_delta",
            "delta":{"stop_reason":if tool {"tool_use"} else {"end_turn"},"stop_sequence":null},
            "usage":{"output_tokens":3}}),
        json!({"type":"message_stop"}),
    ]);
    events
        .into_iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect()
}

pub(super) fn gemini(tool: bool) -> String {
    let parts = if tool {
        json!([{"functionCall":{"id":"stable-call","name":"read","args":{"path":"file"}},
            "thoughtSignature":"c2lnbmVkLXRob3VnaHQ="}])
    } else {
        json!([{"text":"Consider","thought":true},{"text":ANSWER}])
    };
    sse([json!({
        "candidates":[{"content":{"role":"model","parts":parts},
            "finishReason":"STOP","index":0}],
        "usageMetadata":{"promptTokenCount":12,"candidatesTokenCount":3,
            "totalTokenCount":17,"cachedContentTokenCount":8,"thoughtsTokenCount":2},
        "modelVersion":"gemini-3-flash-preview"
    })])
}

pub(super) fn sse(events: impl IntoIterator<Item = Value>) -> String {
    events
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect()
}

pub(super) fn rejection(status: u16, body: Value, headers: &str) -> String {
    let body = body.to_string();
    format!(
        "HTTP/1.1 {status} Fixture rejection\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
        body.len()
    )
}
