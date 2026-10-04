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
    respond_usage(stream, body, fail_summary, text, 100).await
}

pub async fn respond_usage(
    mut stream: TcpStream,
    body: &Value,
    fail_summary: bool,
    text: &str,
    input_tokens: i32,
) -> std::io::Result<()> {
    let summary = is_summary(body);
    if summary && fail_summary {
        stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
        return stream.shutdown().await;
    }
    let messages = body["messages"].as_array().unwrap();
    let read = !summary
        && !messages
            .iter()
            .any(|message| message["role"] == "tool" || message["role"] == "assistant");
    let delta = if read {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"evidence-call","type":"function","function":{"name":plugin_tool("files", "read_file"),"arguments":"{\"path\":\"source.txt\"}"}}]})
    } else {
        json!({"role":"assistant","content":if summary { text } else { "Verified response" }})
    };
    reply_usage(stream, body, delta, read, Some(input_tokens)).await
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
            fail_summary,
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
        json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("loop-{index}"),"type":"function","function":{"name":plugin_tool("files", "read_file"),"arguments":json!({"path":format!("source-{index}.txt")}).to_string()}}]})
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
        .rposition(|message| message["role"] == "user")
        .unwrap();
    let compact = messages[start]["content"]
        .as_str()
        .is_some_and(|text| text.contains("Compact prior evidence"));
    if messages[start]["content"]
        .as_str()
        .is_some_and(|text| text.contains("Compact loop evidence"))
    {
        let completed = messages
            .iter()
            .rev()
            .find(|message| message["role"] == "tool")
            .and_then(|message| message["tool_call_id"].as_str())
            .and_then(|id| id.strip_prefix("native-read-"))
            .and_then(|index| index.parse::<usize>().ok())
            .unwrap_or(0);
        let index = completed + 1;
        let mut calls = Vec::new();
        if completed == 3 {
            calls.push(json!({"index":0,"id":"compact-call","type":"function","function":{"name":"compact_context","arguments":"{}"}}));
        }
        if completed < 4 {
            calls.push(json!({"index":calls.len(),"id":format!("native-read-{index}"),"type":"function","function":{"name":plugin_tool("files", "read_file"),"arguments":json!({"path":format!("source-{index}.txt")}).to_string()}}));
        }
        let call = !calls.is_empty();
        let delta = if call {
            json!({"role":"assistant","tool_calls":calls})
        } else {
            json!({"role":"assistant","content":"Compaction request handled"})
        };
        return reply(stream, body, delta, call).await;
    }
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
