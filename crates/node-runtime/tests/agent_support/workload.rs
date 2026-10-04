//! Paced streaming and a real file tool for native desktop workload measurements.
use super::*;

pub(super) async fn respond(mut stream: TcpStream, body: &Value) -> std::io::Result<()> {
    if compaction::is_summary(body) {
        return compaction::respond(
            stream,
            body,
            false,
            "Retain source.txt and the task constraints",
        )
        .await;
    }
    let messages = body["messages"].as_array().unwrap();
    let seed = messages
        .iter()
        .rev()
        .find(|message| message["role"] == "user")
        .is_some_and(|message| {
            message["content"]
                .as_str()
                .is_some_and(|text| text.starts_with("Seed "))
        });
    let completed = messages
        .iter()
        .rev()
        .take_while(|message| message["role"] != "user")
        .any(|message| message["role"] == "tool");
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        )
        .await?;
    let model = &body["model"];
    for index in 0..if completed { 48 } else { 1 } {
        let delta = if completed {
            json!({"content":format!("Streaming result {index}: preserve source and task constraints 中文 🙂.\n")})
        } else {
            json!({"role":"assistant","tool_calls":[{"index":0,"id":"workload-read","type":"function","function":{"name":plugin_tool("files", "read_file"),"arguments":"{\"path\":\"source.txt\"}"}}]})
        };
        let chunk = json!({"id":"workload","object":"chat.completion.chunk","created":1,"model":model,"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
        stream
            .write_all(format!("data: {chunk}\n\n").as_bytes())
            .await?;
        if completed && !seed {
            tokio::time::sleep(Duration::from_millis(12)).await;
        }
    }
    let chunk = json!({"id":"workload","object":"chat.completion.chunk","created":1,"model":model,"choices":[{"index":0,"delta":{},"finish_reason":if completed {"stop"} else {"tool_calls"}}],"usage":{"prompt_tokens":100,"completion_tokens":100,"total_tokens":200}});
    stream
        .write_all(format!("data: {chunk}\n\ndata: [DONE]\n\n").as_bytes())
        .await?;
    stream.shutdown().await
}
