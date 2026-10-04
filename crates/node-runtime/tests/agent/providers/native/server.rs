use crate::agent_support::plugin_tool;
use sailry_link::CancellationToken;
use sailry_protocol::conversation::ModelApi;
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinSet,
};

pub const KEY: &str = "isolated-native-provider-key";
pub const ANSWER: &str = "原生响应 🙂";

#[derive(Clone)]
pub enum Reply {
    Text,
    Tool,
    Usage,
    Grounding,
    Failure,
    Raw(Arc<Vec<u8>>),
    Redirect(String),
    Wait(Arc<tokio::sync::Notify>),
}

#[derive(Clone, Debug)]
pub struct Request {
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: serde_json::Value,
}

pub struct Server {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<Request>>>,
    stop: CancellationToken,
}

impl Server {
    pub async fn start(api: ModelApi, reply: Reply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let suffix = if api == ModelApi::Anthropic {
            ""
        } else {
            "/v1"
        };
        let endpoint = format!("http://{}{suffix}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let records = requests.clone();
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    _ = stopped.cancelled() => break,
                    _ = tasks.join_next(), if !tasks.is_empty() => {},
                    result = listener.accept() => {
                        let (stream, _) = result.unwrap();
                        let records = records.clone();
                        let reply = reply.clone();
                        tasks.spawn(async move { let _ = respond(stream, records, api, reply).await; });
                    }
                }
            }
            tasks.shutdown().await;
        });
        Self {
            endpoint,
            requests,
            stop,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

async fn respond(
    mut stream: TcpStream,
    records: Arc<Mutex<Vec<Request>>>,
    api: ModelApi,
    reply: Reply,
) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let end = loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() < 2 * 1024 * 1024);
        if let Some(end) = bytes.windows(4).position(|chunk| chunk == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let header = std::str::from_utf8(&bytes[..end]).unwrap();
    let path = header
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned();
    let headers: BTreeMap<_, _> = header
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length: usize = headers["content-length"].parse().unwrap();
    assert!(length < 2 * 1024 * 1024);
    while bytes.len() < end + length {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let body: serde_json::Value = serde_json::from_slice(&bytes[end..end + length]).unwrap();
    let tool = matches!(reply, Reply::Tool) && !body.to_string().contains("工具内容 🙂");
    let search = body["tools"]
        .as_array()
        .is_some_and(|tools| tools.iter().any(|tool| tool["google_search"].is_object()));
    records.lock().unwrap().push(Request {
        path,
        headers,
        body,
    });
    if let Reply::Redirect(target) = &reply {
        stream.write_all(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await?;
        return stream.shutdown().await;
    }
    if matches!(reply, Reply::Failure) {
        stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n").await?;
        stream
            .write_all(
                json!({"type":"error","error":{"type":"api_error", "message":KEY, "code":503}})
                    .to_string()
                    .as_bytes(),
            )
            .await?;
        return stream.shutdown().await;
    }
    if let Reply::Raw(bytes) = &reply {
        for chunk in bytes.chunks(7) {
            stream.write_all(chunk).await?;
        }
        return stream.shutdown().await;
    }
    let content_type = if api == ModelApi::Bedrock {
        "application/json"
    } else {
        "text/event-stream"
    };
    stream
        .write_all(
            format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;
    if let Reply::Wait(closed) = reply {
        let mut byte = [0; 1];
        assert_eq!(stream.read(&mut byte).await?, 0);
        closed.notify_one();
        return Ok(());
    }
    match api {
        ModelApi::DeepSeek | ModelApi::AzureAi | ModelApi::AzureOpenAi => {
            let delta = if tool {
                json!({"reasoning_content":"Inspect the file 🙂", "tool_calls":[{"index":0, "id":"native-call", "type":"function", "function":{"name":plugin_tool("files", "read_file"), "arguments":"{\"path\":\"native.txt\"}"}}]})
            } else {
                json!({"content":ANSWER})
            };
            let usage = if matches!(reply, Reply::Usage) {
                json!({"prompt_tokens":16,"completion_tokens":4,"total_tokens":20,"prompt_cache_hit_tokens":6,"prompt_cache_miss_tokens":10,"prompt_tokens_details":{"cached_tokens":6},"completion_tokens_details":{"reasoning_tokens":1}})
            } else {
                json!({"prompt_tokens":8,"completion_tokens":4,"total_tokens":12})
            };
            let separate_usage = matches!(api, ModelApi::AzureAi | ModelApi::AzureOpenAi);
            let event = json!({"id":"fixture", "object":"chat.completion.chunk", "created":0, "model":"fixture-a", "choices":[{"index":0,"delta":delta,"finish_reason":if tool {"tool_calls"} else {"stop"}}], "usage":if separate_usage { json!(null) } else { usage.clone() }});
            event_bytes(&mut stream, format!("data: {event}\n\n")).await?;
            if separate_usage {
                let event = json!({"id":"fixture", "object":"chat.completion.chunk", "created":0, "model":"fixture-a", "choices":[], "usage":usage});
                event_bytes(&mut stream, format!("data: {event}\n\n")).await?;
            }
            event_bytes(&mut stream, "data: [DONE]\n\n".into()).await?;
        }
        ModelApi::Anthropic => {
            let usage = if matches!(reply, Reply::Usage) {
                json!({"input_tokens":8, "output_tokens":0, "cache_creation_input_tokens":2, "cache_read_input_tokens":6})
            } else {
                json!({"input_tokens":8,"output_tokens":0})
            };
            let block = if tool {
                json!({"type":"tool_use", "id":"native-call", "name":plugin_tool("files", "read_file"), "input":{}})
            } else {
                json!({"type":"text", "text":""})
            };
            let delta = if tool {
                json!({"type":"input_json_delta", "partial_json":json!({"path":"native.txt"}).to_string()})
            } else {
                json!({"type":"text_delta", "text":ANSWER})
            };
            for (name, event) in [
                (
                    "message_start",
                    json!({"type":"message_start", "message":{"id":"native-answer", "type":"message", "role":"assistant", "model":"fixture-a", "content":[], "stop_reason":null, "stop_sequence":null, "usage":usage}}),
                ),
                (
                    "content_block_start",
                    json!({"type":"content_block_start", "index":0, "content_block":block}),
                ),
                (
                    "content_block_delta",
                    json!({"type":"content_block_delta", "index":0, "delta":delta}),
                ),
                (
                    "content_block_stop",
                    json!({"type":"content_block_stop", "index":0}),
                ),
                (
                    "message_delta",
                    json!({"type":"message_delta", "delta":{"stop_reason":if tool { "tool_use" } else { "end_turn" },"stop_sequence":null}, "usage":{"output_tokens":4}}),
                ),
                ("message_stop", json!({"type":"message_stop"})),
            ] {
                event_bytes(&mut stream, format!("event: {name}\ndata: {event}\n\n")).await?;
            }
        }
        ModelApi::Gemini | ModelApi::Vertex => {
            let usage = if matches!(reply, Reply::Usage) {
                json!({"promptTokenCount":16, "candidatesTokenCount":3, "thoughtsTokenCount":1, "cachedContentTokenCount":6, "totalTokenCount":20})
            } else {
                json!({"promptTokenCount":8,"candidatesTokenCount":4,"totalTokenCount":12})
            };
            let part = if tool {
                json!({"functionCall":{"id":"native-call", "name":plugin_tool("files", "read_file"), "args":{"path":"native.txt"}}})
            } else {
                json!({"text":ANSWER})
            };
            let mut event = json!({"candidates":[{"content":{"role":"model", "parts":[part]}, "finishReason":"STOP", "index":0}], "usageMetadata":usage, "modelVersion":"fixture-a"});
            if matches!(reply, Reply::Grounding) && search {
                event["candidates"][0]["groundingMetadata"] = grounding_metadata();
            }
            event_bytes(&mut stream, format!("data: {event}\n\n")).await?;
        }
        ModelApi::Bedrock => {
            let content = if tool {
                json!([{"toolUse":{"toolUseId":"native-call","name":plugin_tool("files", "read_file"),"input":{"path":"native.txt"}}}])
            } else {
                json!([{"text":ANSWER}])
            };
            let usage = if matches!(reply, Reply::Usage) {
                json!({"inputTokens":16,"outputTokens":4,"totalTokens":20,"cacheReadInputTokens":6})
            } else {
                json!({"inputTokens":8,"outputTokens":4,"totalTokens":12})
            };
            stream.write_all(json!({"output":{"message":{"role":"assistant","content":content}}, "stopReason": if tool { "tool_use" } else { "end_turn" }, "usage":usage, "metrics":{"latencyMs":1}}).to_string().as_bytes()).await?;
        }
        _ => unreachable!(),
    }
    stream.shutdown().await
}

async fn event_bytes(stream: &mut TcpStream, event: String) -> std::io::Result<()> {
    // Break a multibyte character across writes to exercise native streaming decoders.
    if let Some(index) = event.find('🙂') {
        stream.write_all(&event.as_bytes()[..index + 1]).await?;
        tokio::time::sleep(Duration::from_millis(2)).await;
        stream.write_all(&event.as_bytes()[index + 1..]).await
    } else {
        stream.write_all(event.as_bytes()).await
    }
}

pub fn grounding_metadata() -> serde_json::Value {
    serde_json::json!({
        "webSearchQueries": ["fixture source"],
        "searchEntryPoint": {
            "renderedContent": "<style>.chip { color: #1a73e8; }</style><a class=\"chip\" href=\"https://www.google.com/search?q=fixture\">Search fixture</a>",
            "sdkBlob": "W1siZml4dHVyZSIsImh0dHBzOi8vd3d3Lmdvb2dsZS5jb20vc2VhcmNoP3E9Zml4dHVyZSJdXQ=="
        },
        "groundingChunks": [{"web": {"uri":"https://example.com/source", "title":"Fixture source"}}],
        "groundingSupports": [{
            "segment": {"partIndex":0, "startIndex":0, "endIndex":ANSWER.len(), "text":ANSWER},
            "groundingChunkIndices":[0]
        }]
    })
}
