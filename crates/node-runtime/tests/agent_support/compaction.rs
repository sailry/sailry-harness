use super::*;

pub const PRIVATE_SENTINEL: &str = "PRIVATE_SUMMARY_PROVIDER_BODY_SENTINEL";

pub fn is_summary(body: &Value) -> bool {
    body["messages"].as_array().is_some_and(|messages| {
        messages.iter().any(|message| {
            message["role"] == "user"
                && message["content"]
                    .as_str()
                    .is_some_and(|text| text.starts_with("Update the existing handoff"))
        })
    })
}

pub fn handoff(text: &str) -> String {
    if text.trim().is_empty() || text.starts_with("## Goals\n") {
        return text.into();
    }
    format!(
        "## Goals\n{text}\n## User constraints\nRetained\n## Key facts\nVerified\n## Changed files and reasons\nNone\n## Errors and fixes\nNone\n## Todo\nContinue\n## Current progress\nKnown\n## Next steps\nResume"
    )
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
    // A handoff is a user message; it replaces the exchange that already read.
    let read = !summary
        && !messages.iter().any(|message| {
            message["role"] == "tool"
                || message["role"] == "assistant"
                || message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("--- End of context handoff ---"))
        });
    let delta = if read {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"evidence-call","type":"function","function":{"name":plugin_tool("files", "read_file"),"arguments":json!({"path":"source.txt","limit":sailry_protocol::MAX_FILE_LINES}).to_string()}}]})
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

pub async fn reject_length(stream: TcpStream, body: &Value) -> std::io::Result<()> {
    reply_finish(
        stream,
        body,
        json!({"role":"assistant", "content":""}),
        false,
        Some(100),
        "length",
    )
    .await
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
        json!({"role":"assistant","tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":plugin_tool("files", "read_file"),"arguments":json!({"path":path,"limit":sailry_protocol::MAX_FILE_LINES}).to_string()}}]})
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
        let text = message["content"].as_str().unwrap_or("");
        message["role"] == "assistant" && text == "Verified all six files"
            || message["role"] == "user"
                && text.contains("--- End of context handoff ---")
                && text.contains("\nVerified all six files\n")
    });
    if is_summary(body) {
        let mut text = text.to_owned();
        if verified {
            text.push_str("\nVerified all six files\n");
        }
        for index in 1..=6 {
            let fact = format!("Evidence {index} 中文 🙂");
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
        .unwrap_or(if verified { 6 } else { 0 });
    let read = completed < 6;
    let index = completed + 1;
    let delta = if read {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("loop-{index}"),"type":"function","function":{"name":plugin_tool("files", "read_file"),"arguments":json!({"path":format!("source-{index}.txt"),"limit":sailry_protocol::MAX_FILE_LINES}).to_string()}}]})
    } else {
        json!({"role":"assistant","content":"Verified all six files"})
    };
    let input_tokens = usage[completed.min(usage.len() - 1)];
    reply_usage(stream, body, delta, read, input_tokens).await
}

/// Explicit compaction is skipped below half the 24k fixture trigger.
pub const NATIVE_USAGE: i32 = 13_000;

pub async fn respond_native(
    stream: TcpStream,
    body: &Value,
    failure_status: Option<u16>,
    text: &str,
) -> std::io::Result<()> {
    if is_summary(body) {
        return respond_usage(stream, body, failure_status, text, 100).await;
    }
    let messages = body["messages"].as_array().unwrap();
    let start = messages
        .iter()
        .rposition(|message| message["role"] == "user");
    let task = start.and_then(|index| messages[index]["content"].as_str());
    let reader = plugin_tool("files", "read_file");
    let retained_read = messages.iter().rev().find_map(|message| {
        if message["role"] != "tool" {
            return None;
        }
        let id = message["tool_call_id"].as_str()?;
        let index = id.strip_prefix("native-read-")?.parse::<usize>().ok()?;
        if !(1..=5).contains(&index) {
            return None;
        }
        messages
            .iter()
            .any(|message| {
                message["role"] == "assistant"
                    && message["tool_calls"].as_array().is_some_and(|calls| {
                        calls
                            .iter()
                            .any(|call| call["id"] == id && call["function"]["name"] == reader)
                    })
            })
            .then_some(index)
    });
    let loop_task = task.is_some_and(|text| text.contains("Compact loop evidence"));
    let protected_growth = task.is_some_and(|text| text.contains("Load protected evidence"));
    let retained_masks = task.is_some_and(|text| text.contains("Mask retained evidence"));
    let continue_work = messages.iter().any(|message| {
        message["role"] == "user"
            && message["content"]
                .as_str()
                .is_some_and(|text| text.contains("Continue after explicit compaction"))
    });
    if loop_task || (start.is_none() && retained_read.is_some()) {
        let completed = match retained_read {
            Some(index) => index,
            None if loop_task => 0,
            None => unreachable!("compacted native loop requires a retained read exchange"),
        };
        let index = completed + 1;
        let mut calls = Vec::new();
        if completed == 3 {
            if protected_growth {
                calls.push(json!({"index":0,"id":"native-skill","type":"function","function":{"name":"load_skill","arguments":json!({"skill":"context-fixture:large"}).to_string()}}));
            }
            calls.push(json!({"index":calls.len(),"id":"compact-call","type":"function","function":{"name":"compact_context","arguments":"{}"}}));
        }
        if completed < if continue_work { 5 } else { 4 } {
            calls.push(json!({"index":calls.len(),"id":format!("native-read-{index}"),"type":"function","function":{"name":plugin_tool("files", "read_file"),"arguments":json!({"path":format!("source-{index}.txt"),"limit":sailry_protocol::MAX_FILE_LINES}).to_string()}}));
        }
        let call = !calls.is_empty();
        let delta = if call {
            json!({"role":"assistant","tool_calls":calls})
        } else {
            json!({"role":"assistant","content":"Compaction request handled"})
        };
        let usage = match completed {
            3 if retained_masks => 30_000,
            3 => NATIVE_USAGE,
            4 if continue_work => 26_000,
            _ => 100,
        };
        return reply_usage(stream, body, delta, call, Some(usage)).await;
    }
    let start = start.expect("native compaction request is missing its user task");
    let compact = task.is_some_and(|text| text.contains("Compact prior evidence"));
    if !compact {
        return respond(stream, body, false, "unused").await;
    }
    let call = !messages[start..]
        .iter()
        .any(|message| message["role"] == "tool");
    let delta = if call {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"compact-call","type":"function","function":{"name":"compact_context","arguments":"{}"}}]})
    } else {
        json!({"role":"assistant","content":"Compaction request handled"})
    };
    let usage = if call { NATIVE_USAGE } else { 100 };
    reply_usage(stream, body, delta, call, Some(usage)).await
}

pub async fn respond_truncated(stream: TcpStream, body: &Value) -> std::io::Result<()> {
    if is_summary(body) {
        reply_finish(
            stream,
            body,
            json!({"role":"assistant","content":"An unfinished handoff"}),
            false,
            Some(100),
            "length",
        )
        .await
    } else {
        respond_usage(stream, body, None, "Unused", 100).await
    }
}

pub async fn respond_summary_tool(stream: TcpStream, body: &Value) -> std::io::Result<()> {
    if is_summary(body) {
        reply_usage(
            stream,
            body,
            json!({"role":"assistant","tool_calls":[{"index":0,"id":"unexpected-summary-tool","type":"function","function":{"name":plugin_tool("files", "write_file"),"arguments":json!({"path":"unexpected-summary.txt","content":"Unexpected summary side effect"}).to_string()}}]}),
            true,
            Some(100),
        )
        .await
    } else {
        respond_usage(stream, body, None, "Unused", 100).await
    }
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
    // Exercise the same ownership envelope as Responses reasoning in the
    // automatic and native compaction loops, through the real model adapter.
    let mut delta = delta;
    if read {
        delta["reasoning_content"] = json!("Inspect the next evidence source");
    }
    let chunk = json!({"id":"context-fixture","object":"chat.completion.chunk","created":1,"model":body["model"],"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let usage = input_tokens.map(
        |input| json!({"prompt_tokens":input,"completion_tokens":10,"total_tokens":input + 10}),
    );
    let end = json!({"id":"context-fixture","object":"chat.completion.chunk","created":1,"model":body["model"],"choices":[{"index":0,"delta":{},"finish_reason":finish}],"usage":usage});
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
        return respond_usage(
            stream,
            body,
            None,
            "Context summary fixture: first file verified",
            100,
        )
        .await;
    }
    let messages = body["messages"].as_array().unwrap();
    let done = |id: &str| {
        messages
            .iter()
            .any(|message| message["role"] == "tool" && message["tool_call_id"] == id)
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
        json!({"role":"assistant","tool_calls":calls.into_iter().enumerate().map(|(index,(id,name,args))| json!({"index":index,"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()})
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
