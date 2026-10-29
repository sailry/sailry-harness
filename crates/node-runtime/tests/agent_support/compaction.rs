use super::*;

pub const PRIVATE_SENTINEL: &str = "PRIVATE_SUMMARY_PROVIDER_BODY_SENTINEL";
const PAIR_SUMMARY: &str = "The tool request and response completed.";

pub fn is_tool_pair(body: &Value) -> bool {
    body["messages"].as_array().is_some_and(|messages| {
        messages.iter().any(|message| {
            matches!(message["role"].as_str(), Some("system" | "developer"))
                && message["content"].as_str().is_some_and(|text| {
                    text.starts_with(
                        "Your task is to summarize a tool call & response pair to save tokens.",
                    )
                })
        })
    })
}

pub(super) fn summarized_pairs(body: &Value) -> usize {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .filter_map(|message| message["content"].as_str())
        .map(|text| text.matches(PAIR_SUMMARY).count())
        .sum()
}

pub(super) async fn respond_pair(stream: TcpStream, body: &Value) -> std::io::Result<()> {
    assert!(body["tools"].as_array().is_none_or(Vec::is_empty));
    assert!(body["tool_choice"].is_null());
    reply_usage(
        stream,
        body,
        json!({"role":"assistant","content":PAIR_SUMMARY}),
        false,
        Some(12),
    )
    .await
}

pub fn is_summary(body: &Value) -> bool {
    body["messages"].as_array().is_some_and(|messages| {
        messages.iter().any(|message| {
            message["role"] == "user" && message["content"].as_str().is_some_and(|text| {
                text == "Please summarize the conversation history provided in the system prompt."
            })
        })
    })
}

pub fn handoff(text: &str) -> String {
    text.into()
}

pub async fn respond(
    stream: TcpStream,
    body: &Value,
    fail_summary: bool,
    text: &str,
) -> std::io::Result<()> {
    respond_usage(stream, body, fail_summary.then_some(400), text, 100).await
}

pub async fn respond_usage(
    mut stream: TcpStream,
    body: &Value,
    failure_status: Option<u16>,
    text: &str,
    input_tokens: i32,
) -> std::io::Result<()> {
    let summary = is_summary(body);
    if summary && let Some(status) = failure_status {
        let body =
            json!({"error":{"message":"Summary service unavailable","type":"server_error","code":"fixture"}, "request":PRIVATE_SENTINEL})
                .to_string();
        stream.write_all(format!("HTTP/1.1 {status} Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
        return stream.shutdown().await;
    }
    let messages = body["messages"].as_array().unwrap();
    // A summary retains the evidence from the exchange that already read.
    let read = !summary
        && !messages.iter().any(|message| {
            message["role"] == "tool"
                || message["role"] == "assistant"
                || message["content"].as_str().is_some_and(|text| {
                    text.contains("Context summary fixture")
                        || text.contains("Verified tool evidence")
                })
        });
    let delta = if read {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"evidence-call","type":"function","function":{"name":"read_file","arguments":json!({"path":"source.txt","limit":sailry_protocol::MAX_FILE_LINES}).to_string()}}]})
    } else {
        json!({"role":"assistant","content":if summary { handoff(text) } else { "Verified response".into() }})
    };
    reply_usage(stream, body, delta, read, Some(input_tokens)).await
}

pub async fn reject_overflow(mut stream: TcpStream) -> std::io::Result<()> {
    let body = json!({"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 70000 tokens > 64000 maximum"}}).to_string();
    stream.write_all(format!("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
    stream.shutdown().await
}

pub async fn respond_recent(
    stream: TcpStream,
    body: &Value,
    id: &str,
    path: &str,
) -> std::io::Result<()> {
    let read = !body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["role"] == "tool" && message["tool_call_id"] == id);
    let delta = if read {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":"read_file","arguments":json!({"path":path,"limit":sailry_protocol::MAX_FILE_LINES}).to_string()}}]})
    } else {
        json!({"role":"assistant","content":"Recent result verified"})
    };
    reply_usage(stream, body, delta, read, Some(100)).await
}

pub async fn respond_plain(
    stream: TcpStream,
    body: &Value,
    input_tokens: i32,
    fail_summary: bool,
) -> std::io::Result<()> {
    if is_summary(body) && fail_summary {
        return respond_usage(stream, body, Some(400), "Unused", input_tokens).await;
    }
    let content = if is_summary(body) {
        handoff("Prepared summary")
    } else {
        "Conversation evidence ".repeat(600)
    };
    reply_usage(
        stream,
        body,
        json!({"role":"assistant","content":content}),
        false,
        Some(input_tokens),
    )
    .await
}

pub async fn respond_loop(
    stream: TcpStream,
    body: &Value,
    fail_summary: bool,
    usage: &[Option<i32>],
    summary_input: i32,
    text: &str,
) -> std::io::Result<()> {
    let messages = body["messages"].as_array().unwrap();
    let verified = messages.iter().any(|message| {
        message["content"]
            .as_str()
            .is_some_and(|text| text.contains("Verified all six files"))
    });
    if is_summary(body) {
        let mut text = text.to_owned();
        if verified {
            text.push_str("\nVerified all six files\n");
        }
        for index in 1..=6 {
            let fact = format!("Evidence {index}");
            if messages.iter().any(|message| {
                message["content"]
                    .as_str()
                    .is_some_and(|content| content.contains(&fact))
            }) {
                text.push('\n');
                text.push_str(&fact);
            }
        }
        return respond_usage(
            stream,
            body,
            fail_summary.then_some(400),
            &text,
            summary_input,
        )
        .await;
    }
    let completed = messages
        .iter()
        .rev()
        .find(|message| message["role"] == "tool")
        .and_then(|message| message["tool_call_id"].as_str())
        .and_then(|id| id.strip_prefix("loop-"))
        .and_then(|index| index.parse::<usize>().ok())
        .unwrap_or_else(|| {
            if verified {
                return 6;
            }
            (1..=6)
                .filter(|index| {
                    messages.iter().any(|message| {
                        message["content"]
                            .as_str()
                            .is_some_and(|text| text.contains(&format!("Evidence {index}")))
                    })
                })
                .max()
                .unwrap_or(0)
        });
    let read = completed < 6;
    let index = completed + 1;
    let delta = if read {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("loop-{index}"),"type":"function","function":{"name":"read_file","arguments":json!({"path":format!("source-{index}.txt"),"limit":sailry_protocol::MAX_FILE_LINES}).to_string()}}]})
    } else {
        json!({"role":"assistant","content":"Verified all six files"})
    };
    let input_tokens = usage[completed.min(usage.len() - 1)];
    reply_usage(stream, body, delta, read, input_tokens).await
}

async fn reply_usage(
    stream: TcpStream,
    body: &Value,
    delta: Value,
    read: bool,
    input_tokens: Option<i32>,
) -> std::io::Result<()> {
    reply_finish(
        stream,
        body,
        delta,
        read,
        input_tokens,
        if read { "tool_calls" } else { "stop" },
    )
    .await
}

async fn reply_finish(
    mut stream: TcpStream,
    body: &Value,
    delta: Value,
    read: bool,
    input_tokens: Option<i32>,
    finish: &str,
) -> std::io::Result<()> {
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        )
        .await?;
    // Exercise reasoning with tool calls through the real model adapter.
    let mut delta = delta;
    if read {
        delta["reasoning_content"] = json!("Inspect the next evidence source");
    }
    let response_id = format!("context-{}", uuid::Uuid::new_v4());
    let chunk = json!({"id":response_id,"object":"chat.completion.chunk","created":1,"model":body["model"],"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let usage = input_tokens.map(
        |input| json!({"prompt_tokens":input,"completion_tokens":10,"total_tokens":input + 10}),
    );
    let end = json!({"id":response_id,"object":"chat.completion.chunk","created":1,"model":body["model"],"choices":[{"index":0,"delta":{},"finish_reason":finish}],"usage":usage});
    stream
        .write_all(format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n").as_bytes())
        .await?;
    stream.shutdown().await
}

pub async fn respond_exchange(
    stream: TcpStream,
    body: &Value,
    delegation: bool,
) -> std::io::Result<()> {
    if is_summary(body) {
        let transcript = body["messages"].to_string();
        let mut text = "Context summary fixture: workflow evidence retained".to_owned();
        for (id, evidence) in [
            ("sequential-1", "source-1.txt"),
            ("sequential-2", "source-2.txt"),
            ("parallel-read", "source-3.txt"),
            ("parallel-write", "Written exactly once"),
            ("delegated-first", "First child evidence"),
            ("delegated-second", "Second child evidence"),
        ] {
            if transcript.contains(evidence) || transcript.contains(&format!("[completed:{id}]")) {
                text.push_str(&format!("\n[completed:{id}] {evidence}"));
            }
        }
        return respond_usage(stream, body, None, &text, 100).await;
    }
    let messages = body["messages"].as_array().unwrap();
    let done = |id: &str| {
        messages.iter().any(|message| {
            message["role"] == "tool" && message["tool_call_id"] == id
                || message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains(&format!("[completed:{id}]")))
        })
    };
    let settled = if delegation {
        done("delegated-first") && done("delegated-second")
    } else {
        done("parallel-read") && done("parallel-write")
    };
    let calls = if settled {
        vec![]
    } else if done("sequential-2") {
        if delegation {
            vec![
                (
                    "delegated-first",
                    plugin_tool("delegation", "spawn_agent"),
                    json!({"role":"review","task":"First child evidence","background":false}),
                ),
                (
                    "delegated-second",
                    plugin_tool("delegation", "spawn_agent"),
                    json!({"role":"fast","task":"Second child evidence","background":false}),
                ),
            ]
        } else {
            vec![
                (
                    "parallel-write",
                    plugin_tool("files", "write_file"),
                    json!({"path":"written.txt","text":"Written exactly once","expected_revision":null}),
                ),
                (
                    "parallel-read",
                    plugin_tool("files", "read_file"),
                    json!({"path":"source-3.txt","limit":sailry_protocol::MAX_FILE_LINES}),
                ),
            ]
        }
    } else {
        let index = if done("sequential-1") { 2 } else { 1 };
        vec![(
            if index == 1 {
                "sequential-1"
            } else {
                "sequential-2"
            },
            plugin_tool("files", "read_file"),
            json!({"path":format!("source-{index}.txt"),"limit":sailry_protocol::MAX_FILE_LINES}),
        )]
    };
    let read = !calls.is_empty();
    let delta = if read {
        json!({"role":"assistant","tool_calls":calls.into_iter().enumerate().map(|(index,(id,name,args))| json!({"index":index,"id":id,"type":"function","function":{"name":offered(body, &name),"arguments":args.to_string()}})).collect::<Vec<_>>()})
    } else {
        json!({"role":"assistant","content":"Verified the independent results"})
    };
    reply_usage(
        stream,
        body,
        delta,
        read,
        Some(if read { 26_000 } else { 100 }),
    )
    .await
}
