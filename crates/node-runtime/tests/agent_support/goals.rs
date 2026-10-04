//! Deterministic responses exercise the real package tools and ADK continuation.
use super::*;

pub(super) async fn respond_loop(
    mut stream: TcpStream,
    body: &Value,
    turns: usize,
) -> std::io::Result<()> {
    let messages = body["messages"].as_array().unwrap();
    assert!(
        messages.iter().any(|message| message["content"]
            .as_str()
            .is_some_and(|text| text.contains("Active goal: "))),
        "goal instructions must come from the package"
    );
    let start = messages
        .iter()
        .rposition(|message| message["role"] == "user")
        .unwrap();
    let recent = messages[start..]
        .iter()
        .filter(|message| message["role"] == "tool")
        .count();
    let continuations = messages
        .iter()
        .filter(|message| {
            message["role"] == "user"
                && message["content"]
                    .as_str()
                    .is_some_and(|text| text.starts_with("Continue working on the active goal."))
        })
        .count();
    let current = messages
        .iter()
        .rev()
        .filter(|message| message["role"] == "tool")
        .find_map(|message| {
            let result: Value = serde_json::from_str(message["content"].as_str()?).ok()?;
            result["goal"].is_object().then(|| result["goal"].clone())
        });
    let call = match recent {
        0 => Some((plugin_tool("goals", "get_goal"), json!({}))),
        1 => Some((
            plugin_tool("files", "read_file"),
            json!({"path":"evidence.txt"}),
        )),
        2 if continuations + 1 >= turns => {
            let goal = current.expect("the model reads the exact package revision");
            Some((
                plugin_tool("goals", "update_goal"),
                json!({"goal_id":goal["id"],"expected_revision":goal["revision"],"description":goal["description"],"state":"completed"}),
            ))
        }
        _ => None,
    };
    let delta = if let Some((name, args)) = &call {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("goal-{continuations}-{recent}"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]})
    } else {
        json!({"role":"assistant","content":"Evidence verified 中文 🙂"})
    };
    send(&mut stream, body, delta, call.is_some()).await
}

pub(super) async fn respond(
    mut stream: TcpStream,
    body: &Value,
    description: &str,
) -> std::io::Result<()> {
    let messages = body["messages"].as_array().unwrap();
    let start = messages
        .iter()
        .rposition(|message| message["role"] == "user")
        .unwrap();
    let recent = messages[start..]
        .iter()
        .filter(|message| message["role"] == "tool")
        .count();
    let call = if recent == 0 {
        Some((
            plugin_tool("goals", "create_goal"),
            json!({"description":description}),
        ))
    } else {
        None
    };
    let delta = if let Some((name, args)) = &call {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"requested-goal","type":"function","function":{"name":name,"arguments":args.to_string()}}]})
    } else {
        json!({"role":"assistant","content":"Goal recorded"})
    };
    send(&mut stream, body, delta, call.is_some()).await
}

pub(super) async fn respond_reasoning(
    mut stream: TcpStream,
    body: &Value,
    turns: usize,
    state: &str,
    count: &std::sync::atomic::AtomicUsize,
) -> std::io::Result<()> {
    use std::sync::atomic::Ordering;
    let messages = body["messages"].as_array().unwrap();
    let active = messages.iter().any(|message| {
        message["content"]
            .as_str()
            .is_some_and(|text| text.contains("Active goal: "))
    });
    let start = messages
        .iter()
        .rposition(|message| message["role"] == "user")
        .unwrap();
    let recent = messages[start..]
        .iter()
        .filter(|message| message["role"] == "tool")
        .count();
    let terminal = if active && recent == 0 {
        (count.fetch_add(1, Ordering::SeqCst) + 1).is_multiple_of(turns)
    } else {
        active && count.load(Ordering::SeqCst).is_multiple_of(turns)
    };
    let call = match recent {
        0 if terminal => Some((plugin_tool("goals", "get_goal"), json!({}))),
        1 if terminal => {
            let result: Value =
                serde_json::from_str(messages.last().unwrap()["content"].as_str().unwrap())
                    .unwrap();
            let goal = &result["goal"];
            Some((
                plugin_tool("goals", "update_goal"),
                json!({"goal_id":goal["id"],"expected_revision":goal["revision"],"description":goal["description"],"state":state}),
            ))
        }
        _ => None,
    };
    let delta = if let Some((name, args)) = &call {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("reasoning-{}-{recent}",count.load(Ordering::SeqCst)),"type":"function","function":{"name":name,"arguments":args.to_string()}}]})
    } else {
        json!({"role":"assistant","content":"Reasoning and writing progress 中文 🙂"})
    };
    send(&mut stream, body, delta, call.is_some()).await
}

async fn send(
    stream: &mut TcpStream,
    body: &Value,
    delta: Value,
    tool: bool,
) -> std::io::Result<()> {
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        )
        .await?;
    let chunk = json!({"id":"goal","object":"chat.completion.chunk","created":1,"model":body["model"],"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let end = json!({"id":"goal","object":"chat.completion.chunk","created":1,"model":body["model"],"choices":[{"index":0,"delta":{},"finish_reason":if tool{"tool_calls"}else{"stop"}}],"usage":{"prompt_tokens":12,"completion_tokens":4,"total_tokens":16}});
    stream
        .write_all(format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n").as_bytes())
        .await?;
    stream.shutdown().await
}
