//! Isolated HTTP MCP peer shared by controller interaction and native-renderer tests.
use super::mcp_peer as peer;
#[allow(unused_imports)]
pub use peer::{alias, form_schema};
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{broadcast, watch},
};
#[path = "mcp_task.rs"]
pub mod tasks;

type Streams = Arc<Mutex<BTreeMap<String, broadcast::Sender<Value>>>>;

pub struct Server {
    pub endpoint: String,
    pub calls: Arc<AtomicUsize>,
    pub initializations: Arc<AtomicUsize>,
    pub streams: Arc<AtomicUsize>,
    pub tasks: Arc<tasks::State>,
    sse: bool,
    stop: CancellationToken,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Server {
    pub async fn task(schema: Value, sse: bool) -> Self {
        Self::with_transport(json!({"task_schema":schema}), sse).await
    }
    pub async fn url(url: &str, sse: bool) -> Self {
        Self::with_transport(json!({"mode":"url", "url":url}), sse).await
    }
    pub async fn start(schema: Value) -> Self {
        Self::with_transport(schema, false).await
    }

    pub async fn with_transport(schema: Value, sse: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let initializations = Arc::new(AtomicUsize::new(0));
        let streams = Arc::new(AtomicUsize::new(0));
        let stop = CancellationToken::new();
        let (shutdown, count) = (stop.clone(), (calls.clone(), initializations.clone()));
        let (answers, _) = watch::channel(Value::Null);
        let events = Streams::default();
        let active = streams.clone();
        let tasks = Arc::new(tasks::State::default());
        let task_state = tasks.clone();
        tokio::spawn(async move {
            let mut work = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    Some(_) = work.join_next(), if !work.is_empty() => {},
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        let (schema, answers, count) = (schema.clone(), answers.clone(), count.clone());
                        let events = sse.then(|| events.clone());
                        let active = active.clone();
                        let tasks = task_state.clone();
                        work.spawn(async move { respond(stream,schema,answers,count,events,active,tasks).await });
                    }
                }
            }
            work.shutdown().await;
        });
        Self {
            endpoint,
            calls,
            initializations,
            streams,
            tasks,
            sse,
            stop,
        }
    }

    pub fn package(&self, root: &Path) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("plugin.json"),json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"example","version":"1.0.0"}).to_string()).unwrap();
        std::fs::write(root.join("mcp.json"),json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"input":{"type":if self.sse { "sse" } else { "streamable-http" },"url":self.endpoint}}}).to_string()).unwrap();
    }
}

async fn respond(
    mut stream: TcpStream,
    schema: Value,
    answers: watch::Sender<Value>,
    (calls, initializations): (Arc<AtomicUsize>, Arc<AtomicUsize>),
    routes: Option<Streams>,
    active: Arc<AtomicUsize>,
    tasks: Arc<tasks::State>,
) -> io::Result<()> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    let end = loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() < 1024 * 1024);
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let head = std::str::from_utf8(&bytes[..end]).unwrap();
    let method = head.split_whitespace().next().unwrap().to_owned();
    let target = head.split_whitespace().nth(1).unwrap().to_owned();
    let headers: BTreeMap<_, _> = head
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .map(|length| length.parse().unwrap())
        .unwrap_or(0);
    while bytes.len() < end + length {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let request: Value = if length == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&bytes[end..end + length]).unwrap()
    };
    if request["method"] == "initialize" {
        initializations.fetch_add(1, Ordering::SeqCst);
    }
    let events = if let Some(routes) = routes {
        if method == "GET" {
            let session = sailry_protocol::RequestId::new().to_string();
            let (events, _) = broadcast::channel(16);
            let mut messages = events.subscribe();
            routes.lock().unwrap().insert(session.clone(), events);
            struct Active {
                count: Arc<AtomicUsize>,
                routes: Streams,
                session: String,
            }
            impl Drop for Active {
                fn drop(&mut self) {
                    self.routes.lock().unwrap().remove(&self.session);
                    self.count.fetch_sub(1, Ordering::SeqCst);
                }
            }
            active.fetch_add(1, Ordering::SeqCst);
            let _active = Active {
                count: active,
                routes,
                session: session.clone(),
            };
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\nevent: endpoint\ndata: /messages?session={session}\n\n").as_bytes()).await?;
            loop {
                tokio::select! {
                    _ = stream.read(&mut buffer) => return Ok(()),
                    message = messages.recv() => send(&mut stream, &None, message.map_err(io::Error::other)?).await?,
                }
            }
        }
        assert_eq!(method, "POST");
        let session = target.strip_prefix("/messages?session=").unwrap();
        let events = routes
            .lock()
            .unwrap()
            .get(session)
            .cloned()
            .ok_or_else(|| io::Error::other("Unknown SSE session"))?;
        stream
            .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await?;
        Some(events)
    } else {
        None
    };
    if let Some(result) = schema
        .get("task_schema")
        .and_then(|schema| tasks.respond(&request, schema, &calls))
    {
        if events.is_some() {
            return send(
                &mut stream,
                &events,
                json!({"jsonrpc":"2.0","id":request["id"],"result":result}),
            )
            .await;
        }
        let body = json!({"jsonrpc":"2.0","id":request["id"],"result":result}).to_string();
        return stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
    }
    if request["method"] == "tools/call" {
        let id = calls.fetch_add(1, Ordering::SeqCst) + 1;
        let mut updates = answers.subscribe();
        if events.is_none() {
            stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await?;
        }
        let params = if schema["mode"] == "url" {
            json!({"mode":"url", "message":"Continue in the service", "url":schema["url"], "elicitationId":format!("url-{id}")})
        } else {
            json!({"mode":"form", "message":"Choose how to prepare the report", "requestedSchema":schema})
        };
        send(&mut stream,&events,json!({"jsonrpc":"2.0","id":format!("form-{id}"),"method":"elicitation/create","params":params})).await?;
        let answer = loop {
            let answer = updates.borrow_and_update().clone();
            if answer["id"] == format!("form-{id}") {
                break answer;
            }
            updates.changed().await.map_err(io::Error::other)?;
        };
        send(&mut stream,&events,json!({"jsonrpc":"2.0","id":request["id"],"result":{"content":[{"type":"text","text":"Input received"}],"structuredContent":answer["result"]}})).await?;
        return Ok(());
    }
    let (status, result) = match (method.as_str(), request["method"].as_str()) {
        ("GET", _) => (405, None),
        ("DELETE", _) => (204, None),
        (_, Some("initialize")) => {
            assert!(request["params"]["capabilities"]["elicitation"]["form"].is_object());
            (
                200,
                Some(
                    json!({"protocolVersion":request["params"]["protocolVersion"],"capabilities":{"tools":{}},"serverInfo":{"name":"input-fixture","version":"1"}}),
                ),
            )
        }
        (_, Some("tools/list")) => (200, Some(json!({"tools":[peer::descriptor("read")]}))),
        (_, None) if request["id"].is_string() => {
            answers.send_replace(request.clone());
            (202, None)
        }
        _ => (202, None),
    };
    if events.is_some() {
        if let Some(result) = result {
            send(
                &mut stream,
                &events,
                json!({"jsonrpc":"2.0","id":request["id"],"result":result}),
            )
            .await?;
        }
        return Ok(());
    }
    let body = result
        .map(|result| json!({"jsonrpc":"2.0","id":request["id"],"result":result}).to_string())
        .unwrap_or_default();
    stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await
}

async fn send(
    stream: &mut TcpStream,
    events: &Option<broadcast::Sender<Value>>,
    message: Value,
) -> io::Result<()> {
    if let Some(events) = events {
        events.send(message).map_err(io::Error::other)?;
        return Ok(());
    }
    stream
        .write_all(format!("event: message\ndata: {message}\n\n").as_bytes())
        .await
}
