use sailry_link::CancellationToken;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinSet,
};

pub mod anthropic;
mod browser;
pub mod compaction;
mod connection_results;
mod delegation;
mod goals;
pub mod phases;
pub mod streaming;
mod terminal;
pub mod web_search;
mod workload;

type Calls = Vec<(String, Value)>;

pub struct Server {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<Value>>>,
    pub authorization: Arc<Mutex<Vec<Option<String>>>>,
    scripted: Option<Arc<Mutex<Calls>>>,
    stop: CancellationToken,
}

#[derive(Clone)]
enum Reply {
    Reasoning,
    Terminal,
    Http {
        failures: usize,
        status: u16,
    },
    Markdown(Arc<String>, Duration, Option<CancellationToken>),
    Workload,
    Browser(Arc<String>, bool),
    Goals(Arc<String>),
    GoalLoop(usize),
    GoalReasoning {
        turns: usize,
        state: &'static str,
        count: Arc<std::sync::atomic::AtomicUsize>,
    },
    Anthropic(anthropic::Reply),
    Compaction {
        failure_status: Option<u16>,
        text: Arc<String>,
        usage: Arc<Vec<i32>>,
    },
    Summary {
        usage: i32,
        fail_summary: bool,
    },
    LoopCompaction {
        fail_summary: bool,
        pending_summary: bool,
        usage: Arc<Vec<Option<i32>>>,
        summary_input: i32,
    },
    ExchangeCompaction {
        delegation: bool,
    },
    NativeCompaction {
        fail_summary: bool,
    },
    WebSearch(Option<Arc<tokio::sync::Notify>>),
    Phases(Option<(String, Value)>),
    EmptyAfterTool,
    EmptyResponse,
    Text {
        slow: bool,
    },
    Tools(Arc<Vec<(String, Value)>>),
    AvailableTools(Arc<Vec<(String, Value)>>),
    ReasonedTools(Arc<Vec<(String, Value)>>),
    TurnTools(Arc<Vec<(String, Value)>>),
    ResultTools(Arc<Vec<(String, Value)>>),
    ScriptedTools(Arc<Mutex<Calls>>),
    PromptTools {
        prompt: Arc<String>,
        calls: Arc<Vec<(String, Value)>>,
    },
    Parallel(Arc<Vec<(String, Value)>>),
    TurnParallel(Arc<Vec<(String, Value)>>),
    Batch(Arc<Vec<(String, Value)>>),
    Gate(Arc<tokio::sync::Barrier>),
    Held(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>),
    Delegation(Option<String>),
    Staged(Arc<Vec<Arc<tokio::sync::Notify>>>),
    ResponsesStaged(streaming::Reply),
    ResponsesParse,
}

impl Server {
    pub async fn reasoning() -> Self {
        Self::serve(Reply::Reasoning).await
    }
    pub async fn terminal() -> Self {
        Self::serve(Reply::Terminal).await
    }
    pub async fn phases_with_tool(name: &str, arguments: Value) -> Self {
        Self::serve(Reply::Phases(Some((name.into(), arguments)))).await
    }

    pub async fn phases() -> Self {
        Self::serve(Reply::Phases(None)).await
    }

    pub async fn empty_after_tool() -> Self {
        Self::serve(Reply::EmptyAfterTool).await
    }

    pub async fn empty_response() -> Self {
        Self::serve(Reply::EmptyResponse).await
    }

    pub async fn staged(gates: Vec<Arc<tokio::sync::Notify>>) -> Self {
        Self::serve(Reply::Staged(Arc::new(gates))).await
    }

    pub async fn staged_responses(
        gates: Vec<Arc<tokio::sync::Notify>>,
        content_type: Option<&str>,
        ending: &'static str,
    ) -> Self {
        Self::serve(Reply::ResponsesStaged(streaming::Reply {
            gates: Arc::new(gates),
            content_type: content_type.map(str::to_owned),
            ending,
        }))
        .await
    }

    pub async fn responses_parse() -> Self {
        Self::serve(Reply::ResponsesParse).await
    }

    pub async fn markdown(text: String) -> Self {
        Self::markdown_after(text, Duration::ZERO).await
    }
    pub async fn markdown_after(text: String, delay: Duration) -> Self {
        Self::serve(Reply::Markdown(Arc::new(text), delay, None)).await
    }
    pub async fn markdown_held(text: String, release: CancellationToken) -> Self {
        Self::serve(Reply::Markdown(
            Arc::new(text),
            Duration::ZERO,
            Some(release),
        ))
        .await
    }
    pub async fn http(failures: usize, status: u16) -> Self {
        Self::serve(Reply::Http { failures, status }).await
    }
    pub async fn held(start: Arc<tokio::sync::Notify>, finish: Arc<tokio::sync::Notify>) -> Self {
        Self::serve(Reply::Held(start, finish)).await
    }

    pub async fn delegation(role: Option<&str>) -> Self {
        Self::serve(Reply::Delegation(role.map(str::to_owned))).await
    }

    pub async fn browser(url: String) -> Self {
        Self::serve(Reply::Browser(Arc::new(url), true)).await
    }
    pub async fn browser_interactions(url: String) -> Self {
        Self::serve(Reply::Browser(Arc::new(url), false)).await
    }
    pub async fn workload() -> Self {
        Self::serve(Reply::Workload).await
    }
    pub async fn file_compaction() -> Self {
        Self::serve(Reply::ExchangeCompaction { delegation: false }).await
    }
    pub async fn delegate_compaction() -> Self {
        Self::serve(Reply::ExchangeCompaction { delegation: true }).await
    }
    pub async fn native_compaction(fail_summary: bool) -> Self {
        Self::serve(Reply::NativeCompaction { fail_summary }).await
    }
    pub async fn loop_compaction(fail_summary: bool) -> Self {
        Self::serve(Reply::LoopCompaction {
            fail_summary,
            pending_summary: false,
            usage: Arc::new(vec![Some(14000)]),
            summary_input: 100,
        })
        .await
    }
    pub async fn pending_loop_compaction() -> Self {
        Self::serve(Reply::LoopCompaction {
            fail_summary: false,
            pending_summary: true,
            usage: Arc::new(vec![Some(14000)]),
            summary_input: 100,
        })
        .await
    }
    pub async fn goals(description: String) -> Self {
        Self::serve(Reply::Goals(Arc::new(description))).await
    }
    pub async fn goal_loop(turns: usize) -> Self {
        Self::serve(Reply::GoalLoop(turns)).await
    }
    pub async fn goal_reasoning(turns: usize, state: &'static str) -> Self {
        assert!(turns > 1 && matches!(state, "completed" | "blocked"));
        Self::serve(Reply::GoalReasoning {
            turns,
            state,
            count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })
        .await
    }
    pub async fn compaction(fail_summary: bool) -> Self {
        Self::compaction_text(
            fail_summary,
            "Context summary fixture: retain user constraints and source.txt evidence 中文 🙂"
                .into(),
        )
        .await
    }
    pub async fn compaction_text(fail_summary: bool, text: String) -> Self {
        Self::compaction_usage(fail_summary, text, 100).await
    }
    pub async fn compaction_usage(fail_summary: bool, text: String, input_tokens: i32) -> Self {
        Self::compaction_sequence(fail_summary, text, vec![input_tokens]).await
    }
    pub async fn compaction_sequence(fail_summary: bool, text: String, usage: Vec<i32>) -> Self {
        assert!(!usage.is_empty());
        Self::serve(Reply::Compaction {
            failure_status: fail_summary.then_some(501),
            text: Arc::new(text),
            usage: Arc::new(usage),
        })
        .await
    }
    pub async fn automatic_compaction(fail_summary: bool) -> Self {
        Self::serve(Reply::Compaction {
            failure_status: fail_summary.then_some(500),
            text: Arc::new(
                "Context summary fixture: retain user constraints and source.txt evidence 中文 🙂"
                    .into(),
            ),
            usage: Arc::new(vec![100, 100, 100, 14000]),
        })
        .await
    }
    pub async fn summary(usage: i32) -> Self {
        Self::serve(Reply::Summary {
            usage,
            fail_summary: false,
        })
        .await
    }
    pub async fn summary_failure(usage: i32) -> Self {
        Self::serve(Reply::Summary {
            usage,
            fail_summary: true,
        })
        .await
    }
    pub async fn loop_usage(usage: Vec<Option<i32>>, summary_input: i32) -> Self {
        assert!(!usage.is_empty());
        Self::serve(Reply::LoopCompaction {
            fail_summary: false,
            pending_summary: false,
            usage: Arc::new(usage),
            summary_input,
        })
        .await
    }
    pub async fn anthropic(reply: anthropic::Reply) -> Self {
        Self::serve(Reply::Anthropic(reply)).await
    }
    pub async fn web_search() -> Self {
        Self::serve(Reply::WebSearch(None)).await
    }

    pub async fn web_search_held(finish: Arc<tokio::sync::Notify>) -> Self {
        Self::serve(Reply::WebSearch(Some(finish))).await
    }

    pub async fn start(slow: bool) -> Self {
        Self::serve(Reply::Text { slow }).await
    }

    pub async fn tools(calls: Vec<(String, Value)>) -> Self {
        Self::serve(Reply::Tools(Arc::new(calls))).await
    }

    pub async fn available_tools(calls: Vec<(String, Value)>) -> Self {
        Self::serve(Reply::AvailableTools(Arc::new(calls))).await
    }

    pub async fn reasoned_tools(calls: Vec<(String, Value)>) -> Self {
        Self::serve(Reply::ReasonedTools(Arc::new(calls))).await
    }

    pub async fn turn_tools(calls: Vec<(String, Value)>) -> Self {
        Self::serve(Reply::TurnTools(Arc::new(calls))).await
    }

    pub async fn result_tools(calls: Vec<(String, Value)>) -> Self {
        Self::serve(Reply::ResultTools(Arc::new(calls))).await
    }

    pub async fn scripted_tools(calls: Vec<(String, Value)>) -> Self {
        let calls = Arc::new(Mutex::new(calls));
        let mut server = Self::serve(Reply::ScriptedTools(calls.clone())).await;
        server.scripted = Some(calls);
        server
    }

    pub fn replace_tools(&self, calls: Vec<(String, Value)>) {
        *self
            .scripted
            .as_ref()
            .expect("scripted tools expected")
            .lock()
            .unwrap() = calls;
    }

    pub async fn prompt_tools(prompt: String, calls: Vec<(String, Value)>) -> Self {
        Self::serve(Reply::PromptTools {
            prompt: Arc::new(prompt),
            calls: Arc::new(calls),
        })
        .await
    }

    pub async fn parallel(calls: Vec<(String, Value)>) -> Self {
        Self::serve(Reply::Parallel(Arc::new(calls))).await
    }

    pub async fn turn_parallel(calls: Vec<(String, Value)>) -> Self {
        Self::serve(Reply::TurnParallel(Arc::new(calls))).await
    }

    pub async fn batch(calls: Vec<(String, Value)>) -> Self {
        Self::serve(Reply::Batch(Arc::new(calls))).await
    }

    pub async fn gate(count: usize) -> Self {
        Self::serve(Reply::Gate(Arc::new(tokio::sync::Barrier::new(count)))).await
    }

    async fn serve(reply: Reply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let records = requests.clone();
        let authorization = Arc::new(Mutex::new(Vec::new()));
        let headers = authorization.clone();
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    _ = stopped.cancelled() => break,
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        let records = records.clone();
                        let headers = headers.clone();
                        let reply = reply.clone();
                        tasks.spawn(async move { let _ = respond(stream, records, headers, reply).await; });
                    }
                    _ = tasks.join_next(), if !tasks.is_empty() => {}
                }
            }
            tasks.shutdown().await;
        });
        Self {
            endpoint,
            requests,
            authorization,
            scripted: None,
            stop,
        }
    }

    pub async fn wait_count(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while self.requests.lock().unwrap().len() < count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("model request deadline");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

async fn respond(
    mut stream: TcpStream,
    records: Arc<Mutex<Vec<Value>>>,
    authorization: Arc<Mutex<Vec<Option<String>>>>,
    reply: Reply,
) -> std::io::Result<()> {
    let mut request = Vec::new();
    let mut buffer = [0; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        request.extend_from_slice(&buffer[..count]);
        assert!(request.len() < 2 * 1024 * 1024);
        if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let headers = std::str::from_utf8(&request[..header_end]).unwrap();
    if headers.starts_with("GET ") {
        stream
            .write_all(
                b"HTTP/1.1 426 Upgrade Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await?;
        return stream.shutdown().await;
    }
    let responses = headers.starts_with("POST /v1/responses ");
    let media = headers.starts_with("POST /v1/images/generations ")
        || headers.starts_with("POST /v1/videos ");
    authorization.lock().unwrap().push(
        headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.trim().to_owned()),
    );
    let length: usize = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .unwrap()
        .1
        .trim()
        .parse()
        .unwrap();
    while request.len() < header_end + length {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        request.extend_from_slice(&buffer[..count]);
    }
    let body: Value = serde_json::from_slice(&request[header_end..header_end + length]).unwrap();
    let vision = matches!(reply, Reply::Tools(_) | Reply::Held(..))
        && body["stream"] != true
        && body["messages"].as_array().is_some_and(|messages| {
            messages.iter().any(|message| {
                message["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().any(|part| part["type"] == "image_url"))
            })
        });
    if media || vision {
        records.lock().unwrap().push(body.clone());
        let body = if vision {
            r#"{"error":{"code":"unsupported_capability","message":"Fixture model does not accept image inputs"}}"#
        } else {
            r#"{"error":{"code":"unsupported_model","message":"Fixture model does not generate media"}}"#
        };
        stream.write_all(format!("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
        return stream.shutdown().await;
    }
    let model = body["model"].as_str().unwrap().to_owned();
    if let Reply::Http { failures, status } = reply {
        let count = records.lock().unwrap().len();
        if count < failures {
            records.lock().unwrap().push(body.clone());
            let body = r#"{"error":{"message":"fixture unavailable","type":"server_error","code":"fixture"}}"#;
            stream.write_all(format!("HTTP/1.1 {status} Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await?;
            return Ok(());
        }
    }

    if let Reply::Workload = reply {
        records.lock().unwrap().push(body.clone());
        return workload::respond(stream, &body).await;
    }
    if let Reply::Goals(description) = reply {
        records.lock().unwrap().push(body.clone());
        return goals::respond(stream, &body, &description).await;
    }
    if let Reply::GoalLoop(turns) = reply {
        records.lock().unwrap().push(body.clone());
        return goals::respond_loop(stream, &body, turns).await;
    }
    if let Reply::GoalReasoning {
        turns,
        state,
        count,
    } = reply
    {
        records.lock().unwrap().push(body.clone());
        return goals::respond_reasoning(stream, &body, turns, state, &count).await;
    }
    if let Reply::Compaction {
        failure_status,
        text,
        usage,
    } = reply
    {
        let index = {
            let mut records = records.lock().unwrap();
            let index = records
                .iter()
                .filter(|request| !compaction::is_summary(request))
                .count();
            records.push(body.clone());
            index
        };
        let input_tokens = if compaction::is_summary(&body) {
            100
        } else {
            usage[index.min(usage.len() - 1)]
        };
        return compaction::respond_usage(stream, &body, failure_status, &text, input_tokens).await;
    }
    if let Reply::Summary {
        usage,
        fail_summary,
    } = reply
    {
        records.lock().unwrap().push(body.clone());
        return compaction::respond_plain(stream, &body, usage, fail_summary).await;
    }
    if let Reply::LoopCompaction {
        fail_summary,
        pending_summary,
        usage,
        summary_input,
    } = reply
    {
        records.lock().unwrap().push(body.clone());
        if pending_summary && compaction::is_summary(&body) {
            return std::future::pending().await;
        }
        return compaction::respond_loop(stream, &body, fail_summary, &usage, summary_input).await;
    }
    if let Reply::ExchangeCompaction { delegation } = reply {
        records.lock().unwrap().push(body.clone());
        return compaction::respond_exchange(stream, &body, delegation).await;
    }
    if let Reply::NativeCompaction { fail_summary } = reply {
        records.lock().unwrap().push(body.clone());
        return compaction::respond_native(stream, &body, fail_summary).await;
    }
    if let Reply::Anthropic(reply) = reply {
        records.lock().unwrap().push(body.clone());
        return anthropic::respond(stream, &body, reply).await;
    }
    if matches!(reply, Reply::Terminal) {
        records.lock().unwrap().push(body.clone());
        return terminal::respond(stream, &body, responses).await;
    }
    if responses {
        if matches!(reply, Reply::ResponsesParse) {
            records.lock().unwrap().push(body);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                )
                .await?;
            let event = json!({
                "type":"response.output_text.delta",
                "sequence_number":0,
                "item_id":"msg",
                "content_index":0,
                "delta":"PRIVATE_SENTINEL"
            });
            stream
                .write_all(format!("data: {event}\n\n").as_bytes())
                .await?;
            return stream.shutdown().await;
        }
        if let Reply::ResponsesStaged(reply) = reply {
            records.lock().unwrap().push(body.clone());
            return streaming::respond(stream, &body, reply).await;
        }
        if matches!(reply, Reply::EmptyAfterTool) {
            records.lock().unwrap().push(body.clone());
            return phases::respond_empty(stream, &body).await;
        }
        if let Reply::Phases(tool) = reply {
            records.lock().unwrap().push(body.clone());
            return phases::respond(stream, &body, tool).await;
        }
        if let Reply::WebSearch(finish) = reply {
            records.lock().unwrap().push(body.clone());
            return web_search::respond(stream, &body, finish).await;
        }
        records.lock().unwrap().push(body);
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await?;
        if matches!(reply, Reply::Reasoning) {
            let delta = json!({"type":"response.reasoning_summary_text.delta", "sequence_number":0,
                "item_id":"thinking-fixture", "output_index":0,"summary_index":0,
                "delta":"Retained reasoning without a final answer"});
            stream
                .write_all(format!("data: {delta}\n\n").as_bytes())
                .await?;
        }
        let mut event = json!({"type":"response.completed", "sequence_number":0, "response":{
            "id":"response-fixture", "object":"response", "created_at":1, "status":"completed", "model":model,
            "output":[{"type":"message", "id":"message-fixture", "role":"assistant", "status":"completed",
                "content":[{"type":"output_text", "text":format!("answer-{model}"), "annotations":[]}]}]
        }});
        if matches!(reply, Reply::Reasoning) {
            event["response"]["output"] = json!([{"type":"reasoning", "id":"thinking-fixture", "summary":[
                {"type":"summary_text","text":"Retained reasoning without a final answer"}]}]);
        } else if matches!(reply, Reply::EmptyResponse) {
            event["response"]["output"] = json!([]);
        }
        stream
            .write_all(format!("data: {event}\n\ndata: [DONE]\n\n").as_bytes())
            .await?;
        return stream.shutdown().await;
    }
    let completed = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        // Turn fixtures submit text prompts. Tool image companions use
        // user-role arrays and must not restart the scripted tool sequence.
        .take_while(|message| {
            !matches!(
                reply,
                Reply::TurnTools(_)
                    | Reply::ResultTools(_)
                    | Reply::ScriptedTools(_)
                    | Reply::PromptTools { .. }
                    | Reply::TurnParallel(_)
            ) || message["role"] != "user"
                || !message["content"].is_string()
        })
        .filter(|message| message["role"] == "tool")
        .count();
    let browser_call = if let Reply::Browser(url, capture) = &reply {
        browser::next(&body, url, *capture)
    } else {
        None
    };
    let delegated_call = if let Reply::Delegation(role) = &reply {
        delegation::next(&body, role.as_deref())
    } else {
        None
    };
    let scripted = match &reply {
        Reply::ScriptedTools(calls) => Some(calls.lock().unwrap().clone()),
        _ => None,
    };
    let resolved = if let Reply::ResultTools(calls) = &reply {
        calls
            .get(completed)
            .map(|call| connection_results::resolve(&body, call))
    } else {
        None
    };
    let available = if let Reply::AvailableTools(calls) = &reply {
        calls
            .iter()
            .filter(|(name, _)| {
                body["tools"].as_array().is_some_and(|tools| {
                    tools
                        .iter()
                        .any(|tool| tool["function"]["name"].as_str() == Some(name.as_str()))
                })
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let call = match &reply {
        Reply::AvailableTools(_) => available.get(completed).copied(),
        Reply::Tools(calls) | Reply::ReasonedTools(calls) | Reply::TurnTools(calls) => {
            calls.get(completed)
        }
        Reply::ScriptedTools(_) => scripted.as_ref().unwrap().get(completed),
        Reply::ResultTools(_) => resolved.as_ref(),
        Reply::PromptTools { prompt, calls } => body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|message| message["role"] == "user" && message["content"].is_string())
            .is_some_and(|message| {
                message["content"]
                    .as_str()
                    .unwrap()
                    .strip_suffix(prompt.as_str())
                    .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('\n'))
            })
            .then(|| calls.get(completed))
            .flatten(),
        Reply::Delegation(_) => delegated_call.as_ref(),
        _ => browser_call.as_ref(),
    };
    let batch = matches!(&reply, Reply::Batch(_) if completed == 0)
        || (matches!(&reply, Reply::Parallel(_) | Reply::TurnParallel(_) if completed == 0)
            && body["tools"].as_array().is_some_and(|tools| {
                tools.iter().any(|tool| {
                    tool["function"]["name"] == plugin_tool("delegation", "spawn_agent")
                })
            }));
    let delta = match (&reply, batch, call) {
        (Reply::Staged(_), _, _) => json!({"role":"assistant", "content":"First"}),
        (Reply::Markdown(text, ..), _, _) => json!({"role":"assistant", "content":text.as_str()}),
        (Reply::Parallel(calls) | Reply::TurnParallel(calls) | Reply::Batch(calls), true, _) => {
            json!({"role": "assistant", "tool_calls": calls.iter().enumerate().map(|(index, (name, args))|
                json!({"index": index, "id": format!("call-fixture-{index}"), "type": "function", "function": {"name": name, "arguments": args.to_string()}})
            ).collect::<Vec<_>>()})
        }
        (_, _, Some((name, arguments))) => {
            json!({"role": "assistant", "tool_calls": [{"index": 0, "id": format!("call-fixture-{completed}"), "type": "function", "function": {"name": name, "arguments": arguments.to_string()}}]})
        }
        _ => {
            json!({"role": "assistant", "content": if matches!(reply, Reply::Text { slow: true }) { "partial-fixture".to_owned() } else { format!("answer-{model}") }})
        }
    };
    records.lock().unwrap().push(body);
    if let Reply::Markdown(_, delay, release) = &reply {
        tokio::time::sleep(*delay).await;
        if let Some(release) = release {
            release.cancelled().await;
        }
    }
    if let Reply::Held(start, _) = &reply {
        start.notified().await;
    }
    if let Reply::Gate(barrier) = &reply {
        tokio::time::timeout(Duration::from_secs(10), barrier.wait())
            .await
            .expect("parallel model calls deadline");
    }
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        )
        .await?;
    let chunk = json!({"id":"completion-fixture","object":"chat.completion.chunk","created":1,"model":model,"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    if matches!(reply, Reply::ReasonedTools(_)) {
        let thinking = json!({"id":"completion-fixture","object":"chat.completion.chunk","created":1,"model":model,"choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":format!("Reasoning step {completed}")},"finish_reason":null}]});
        stream
            .write_all(format!("data: {thinking}\n\n").as_bytes())
            .await?;
    }
    stream
        .write_all(format!("data: {chunk}\n\n").as_bytes())
        .await?;
    if let Reply::Staged(gates) = &reply {
        for (index, text) in [" second", " third"].iter().enumerate() {
            gates[index].notified().await;
            let chunk = json!({"id":"completion-fixture","object":"chat.completion.chunk","created":1,"model":model,"choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]});
            stream
                .write_all(format!("data: {chunk}\n\n").as_bytes())
                .await?;
        }
        gates[2].notified().await;
    }
    if matches!(reply, Reply::Text { slow: true }) {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
    if let Reply::Held(_, finish) = &reply {
        finish.notified().await;
    }
    let final_chunk = json!({"id":"completion-fixture","object":"chat.completion.chunk","created":1,"model":model,"choices":[{"index":0,"delta":{},"finish_reason":if call.is_some() || batch { "tool_calls" } else { "stop" }}],"usage":{"prompt_tokens":12,"completion_tokens":4,"total_tokens":16}});
    stream
        .write_all(format!("data: {final_chunk}\n\ndata: [DONE]\n\n").as_bytes())
        .await?;
    stream.shutdown().await
}

/// Stable public tool namespace used by ordinary packages.
pub fn plugin_tool(package: &str, tool: &str) -> String {
    let digest = blake3::hash(package.as_bytes()).to_hex();
    format!("plugin_{}_{}", &digest[..16], tool)
}
