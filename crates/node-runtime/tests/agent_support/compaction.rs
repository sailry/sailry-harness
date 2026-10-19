use super::*;

pub fn is_summary(body: &Value) -> bool {
    body["messages"].as_array().is_some_and(|messages| {
        messages.iter().any(|message| {
            message["role"] == "system"
                && message["content"]
                    .as_str()
                    .is_some_and(|text| text.starts_with("Summarize this conversation"))
        })
    })
}

pub async fn respond(
    stream: TcpStream,
    body: &Value,
    fail_summary: bool,
    text: &str,
) -> std::io::Result<()> {
    respond_usage(stream, body, fail_summary.then_some(501), text, 100).await
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
        stream
            .write_all(
                format!(
                    "HTTP/1.1 {status} Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await?;
        return stream.shutdown().await;
    }
    let messages = body["messages"].as_array().unwrap();
    let read = !summary
        && !messages
            .iter()
            .any(|message| message["role"] == "tool" || message["role"] == "assistant");
    let delta = if read {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"evidence-call","type":"function","function":{"name":plugin_tool("files", "read_file"),"arguments":json!({"path":"source.txt","limit":sailry_protocol::MAX_FILE_LINES}).to_string()}}]})
    } else {
        json!({"role":"assistant","content":if summary { text } else { "Verified response" }})
    };
    reply_usage(stream, body, delta, read, Some(input_tokens)).await
}

pub async fn respond_plain(
    stream: TcpStream,
    body: &Value,
    input_tokens: i32,
    fail_summary: bool,
) -> std::io::Result<()> {
    if is_summary(body) && fail_summary {
        return respond_usage(stream, body, Some(501), "Unused", input_tokens).await;
    }
    let content = if is_summary(body) {
        "Prepared summary".into()
    } else {
        "Conversation evidence ".repeat(200)
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
) -> std::io::Result<()> {
    if is_summary(body) {
        return respond_usage(
            stream,
            body,
            fail_summary.then_some(501),
            "Context summary fixture: earlier tool evidence retained",
            summary_input,
        )
        .await;
    }
    let completed = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| message["role"] == "tool")
        .and_then(|message| message["tool_call_id"].as_str())
        .and_then(|id| id.strip_prefix("loop-"))
        .and_then(|index| index.parse::<usize>().ok())
        .unwrap_or(0);
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

pub async fn respond_native(
    stream: TcpStream,
    body: &Value,
    fail_summary: bool,
) -> std::io::Result<()> {
    if is_summary(body) {
        return respond(
            stream,
            body,
            fail_summary,
            "Context summary fixture: original evidence retained",
        )
        .await;
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
        if !(1..=4).contains(&index) {
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
    if loop_task || (start.is_none() && retained_read.is_some()) {
        let completed = match retained_read {
            Some(index) => index,
            None if loop_task => 0,
            None => unreachable!("compacted native loop requires a retained read exchange"),
        };
        let index = completed + 1;
        let mut calls = Vec::new();
        if completed == 3 {
            calls.push(json!({"index":0,"id":"compact-call","type":"function","function":{"name":"compact_context","arguments":"{}"}}));
        }
        if completed < 4 {
            calls.push(json!({"index":calls.len(),"id":format!("native-read-{index}"),"type":"function","function":{"name":plugin_tool("files", "read_file"),"arguments":json!({"path":format!("source-{index}.txt"),"limit":sailry_protocol::MAX_FILE_LINES}).to_string()}}));
        }
        let call = !calls.is_empty();
        let delta = if call {
            json!({"role":"assistant","tool_calls":calls})
        } else {
            json!({"role":"assistant","content":"Compaction request handled"})
        };
        return reply(stream, body, delta, call).await;
    }
    let start = start.expect("native compaction request is missing its user task");
    let compact = task.is_some_and(|text| text.contains("Compact prior evidence"));
    if !compact {
        return respond(stream, body, fail_summary, "unused").await;
    }
    let call = !messages[start..]
        .iter()
        .any(|message| message["role"] == "tool");
    let delta = if call {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"compact-call","type":"function","function":{"name":"compact_context","arguments":"{}"}}]})
    } else {
        json!({"role":"assistant","content":"Compaction request handled"})
    };
    reply(stream, body, delta, call).await
}

async fn reply(stream: TcpStream, body: &Value, delta: Value, read: bool) -> std::io::Result<()> {
    reply_usage(stream, body, delta, read, Some(100)).await
}

async fn reply_usage(
    mut stream: TcpStream,
    body: &Value,
    delta: Value,
    read: bool,
    input_tokens: Option<i32>,
) -> std::io::Result<()> {
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        )
        .await?;
    let chunk = json!({"id":"context-fixture","object":"chat.completion.chunk","created":1,"model":body["model"],"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let usage = input_tokens.map(
        |input| json!({"prompt_tokens":input,"completion_tokens":10,"total_tokens":input + 10}),
    );
    let end = json!({"id":"context-fixture","object":"chat.completion.chunk","created":1,"model":body["model"],"choices":[{"index":0,"delta":{},"finish_reason":if read { "tool_calls" } else { "stop" }}],"usage":usage});
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
        Some(if read { 14_000 } else { 100 }),
    )
    .await
}
