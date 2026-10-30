use super::*;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

#[derive(Default)]
struct ServerState {
    statuses: VecDeque<StatusCode>,
    calls: Vec<(HeaderMap, Value)>,
    hold: Option<Arc<tokio::sync::Notify>>,
}

struct Server {
    endpoint: String,
    state: Arc<Mutex<ServerState>>,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    async fn new(statuses: Vec<StatusCode>) -> Self {
        let state = Arc::new(Mutex::new(ServerState {
            statuses: statuses.into(),
            calls: Vec::new(),
            hold: None,
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/log", listener.local_addr().unwrap());
        let received = state.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let (headers, value) = read(&mut socket).await;
                let (status, hold) = {
                    let mut state = received.lock().unwrap();
                    state.calls.push((headers, value));
                    (
                        state.statuses.pop_front().unwrap_or(StatusCode::OK),
                        state.hold.clone(),
                    )
                };
                if let Some(hold) = hold {
                    hold.notified().await;
                }
                let body = "PRIVATE_UPSTREAM_BODY";
                let reply = format!(
                    "HTTP/1.1 {} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    status.as_u16(),
                    body.len()
                );
                // Cancellation can close an observed request before this response.
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        Self {
            endpoint,
            state,
            task,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read(socket: &mut tokio::net::TcpStream) -> (HeaderMap, Value) {
    let mut bytes = Vec::new();
    let end = loop {
        let mut part = [0; 4096];
        let count = socket.read(&mut part).await.unwrap();
        assert!(count > 0 && bytes.len() < 65536);
        bytes.extend_from_slice(&part[..count]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let header = std::str::from_utf8(&bytes[..end]).unwrap();
    let mut lines = header.lines();
    assert!(lines.next().unwrap().starts_with("POST /log "));
    let mut headers = HeaderMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value.trim()).unwrap(),
            );
        }
    }
    let length = headers[reqwest::header::CONTENT_LENGTH]
        .to_str()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert!(length <= 1024 * 1024);
    while bytes.len() < end + length {
        let mut part = [0; 4096];
        let count = socket.read(&mut part).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&part[..count]);
    }
    (
        headers,
        serde_json::from_slice(&bytes[end..end + length]).unwrap(),
    )
}

fn batch() -> Batch {
    Batch {
        table: "turns",
        data: vec![json!({"event_id":"stable-id","count":2,"result":"completed"})],
    }
}

#[tokio::test]
async fn preserves_ids_and_auth_across_retries() {
    let server = Server::new(vec![
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::BAD_GATEWAY,
        StatusCode::OK,
    ])
    .await;
    Transport::at("synthetic-key", &server.endpoint)
        .unwrap()
        .send(&batch(), None)
        .await;
    let state = server.state.lock().unwrap();
    assert_eq!(state.calls.len(), 3);
    for (headers, body) in &state.calls {
        assert_eq!(headers[AUTHORIZATION], "Bearer synthetic-key");
        assert_eq!(
            body,
            &json!({"table":"turns","data":[{"event_id":"stable-id","count":2,"result":"completed"}]})
        );
        assert!(!body.to_string().contains("synthetic-key"));
    }
}

#[tokio::test]
async fn never_retries_permanent_failures() {
    for status in [400, 401, 403, 404, 408, 500, 501] {
        let server = Server::new(vec![StatusCode::from_u16(status).unwrap()]).await;
        Transport::at("synthetic-key", &server.endpoint)
            .unwrap()
            .send(&batch(), None)
            .await;
        assert_eq!(server.state.lock().unwrap().calls.len(), 1);
    }
    for status in [429, 502, 503, 504] {
        assert!(retryable(StatusCode::from_u16(status).unwrap()));
    }
}

#[tokio::test]
async fn limits_retries_and_exit_deadline() {
    let server = Server::new(vec![StatusCode::SERVICE_UNAVAILABLE; 5]).await;
    Transport::at("synthetic-key", &server.endpoint)
        .unwrap()
        .send(&batch(), None)
        .await;
    assert_eq!(server.state.lock().unwrap().calls.len(), ATTEMPTS as usize);
    let server = Server::new(vec![StatusCode::SERVICE_UNAVAILABLE; 5]).await;
    let deadline = Instant::now() + Duration::from_millis(50);
    Transport::at("synthetic-key", &server.endpoint)
        .unwrap()
        .send(&batch(), Some(deadline))
        .await;
    assert_eq!(server.state.lock().unwrap().calls.len(), 1);
    assert!(Instant::now() < deadline + Duration::from_millis(100));
}

#[test]
fn rejects_invalid_auth_without_details() {
    assert!(Transport::at("synthetic\nkey", "http://127.0.0.1:1/log").is_none());
    assert_eq!(backoff(0), Duration::from_millis(250));
    assert_eq!(backoff(2), Duration::from_secs(1));
}

#[test]
fn initializes_tls_independently() {
    assert!(Transport::at("synthetic-key", ENDPOINT).is_some());
    assert!(rustls::crypto::CryptoProvider::get_default().is_some());
}

async fn observed(server: &Server, count: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if server.state.lock().unwrap().calls.len() >= count {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn shutdown_drains_all_five_tables() {
    use super::super::{
        Control, Envelope, Observation, RequestResult, Shutdown, StatusClass, Tokens, Tool,
        ToolResult, TurnResult, worker,
    };
    use sailry_protocol::{WorkMode, search::Driver};
    use tokio::sync::{Notify, mpsc, oneshot};
    let server = Server::new(vec![]).await;
    let control = Control::fixture(false, true);
    let (records, received) = mpsc::channel(16);
    let (shutdown, closing) = mpsc::channel(1);
    let model=serde_json::from_value(json!({"id":sailry_protocol::ProviderId::new(),"revision":1,"name":"PRIVATE_NAME","api":"responses","authentication":"api_key","endpoint":"https://private.invalid","enabled":true,"default_model":"PRIVATE_MODEL","credential":null})).unwrap();
    for observation in [
        Observation::Launch,
        Observation::Turn {
            result: TurnResult::Completed,
            mode: WorkMode::Code,
            subagent: false,
            error: None,
        },
        Observation::Tool {
            tool: Tool::builtin("read_session").unwrap(),
            result: ToolResult::Success,
            error: None,
            elapsed: Duration::from_millis(2),
        },
        Observation::Model {
            model: super::super::Model::capture(&model, "PRIVATE_MODEL"),
            result: RequestResult::Success,
            status: StatusClass::Success,
            error: None,
            retries: None,
            tokens: Tokens {
                input: 4,
                output: 2,
                cache_read: 1,
                cache_write: 0,
            },
        },
        Observation::Search {
            driver: Some(Driver::Exa),
            result: RequestResult::Success,
            error: None,
            elapsed: Duration::from_millis(3),
        },
    ] {
        records
            .send(Envelope {
                epoch: control.epoch(),
                observation,
            })
            .await
            .unwrap();
    }
    let (reply, ack) = oneshot::channel();
    let deadline = Instant::now() + Duration::from_secs(1);
    shutdown.send(Shutdown { deadline, reply }).await.unwrap();
    let transport = Transport::at("synthetic-key", &server.endpoint).unwrap();
    let task = tokio::spawn(worker(
        control,
        received,
        closing,
        Arc::new(Notify::new()),
        transport,
    ));
    tokio::time::timeout_at(deadline, ack)
        .await
        .unwrap()
        .unwrap();
    task.await.unwrap();
    let state = server.state.lock().unwrap();
    assert_eq!(state.calls.len(), 5);
    let tables: Vec<_> = state
        .calls
        .iter()
        .map(|(_, body)| body["table"].as_str().unwrap())
        .collect();
    assert_eq!(
        tables,
        vec![
            "app_launches",
            "turns",
            "tool_calls",
            "model_requests",
            "search_requests"
        ]
    );
    for (_, body) in &state.calls {
        assert_eq!(body["data"][0]["count"], 1);
        assert!(!body.to_string().contains("PRIVATE_"));
        assert!(!body.to_string().contains("synthetic-key"));
    }
}

#[tokio::test]
async fn opt_out_discards_queued_windows() {
    use super::super::{Control, Envelope, Observation, Shutdown, worker};
    use tokio::sync::{Notify, mpsc, oneshot};
    let server = Server::new(vec![]).await;
    let control = Control::fixture(false, true);
    let epoch = control.epoch();
    let (records, received) = mpsc::channel(16);
    let (shutdown, closing) = mpsc::channel(1);
    records
        .send(Envelope {
            epoch,
            observation: Observation::Launch,
        })
        .await
        .unwrap();
    control.fixture_set(false, false);
    let (reply, ack) = oneshot::channel();
    let deadline = Instant::now() + Duration::from_secs(1);
    shutdown.send(Shutdown { deadline, reply }).await.unwrap();
    worker(
        control,
        received,
        closing,
        Arc::new(Notify::new()),
        Transport::at("synthetic-key", &server.endpoint).unwrap(),
    )
    .await;
    ack.await.unwrap();
    assert!(server.state.lock().unwrap().calls.is_empty());
}

#[tokio::test]
async fn opt_out_cancels_an_active_send() {
    use super::super::{Control, Observation, aggregate::Window, flush};
    use tokio::sync::Notify;
    let server = Server::new(vec![]).await;
    let hold = Arc::new(Notify::new());
    server.state.lock().unwrap().hold = Some(hold.clone());
    let control = Control::fixture(false, true);
    let mut window = Window::new(control.epoch(), control.metadata(), chrono::Utc::now());
    window.add(Observation::Launch);
    let owned = control.clone();
    let transport = Transport::at("synthetic-key", &server.endpoint).unwrap();
    let task = tokio::spawn(async move {
        flush(&owned, Some(window), &transport, &Notify::new(), None).await;
    });
    observed(&server, 1).await;
    control.fixture_set(false, false);
    tokio::time::timeout(Duration::from_millis(100), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(server.state.lock().unwrap().calls.len(), 1);
    hold.notify_waiters();
}

#[tokio::test]
async fn shutdown_retains_unsent_batches() {
    use super::super::{Control, Observation, Pending, TurnResult, aggregate::Window, deliver};
    use sailry_protocol::WorkMode;
    use tokio::sync::Notify;
    let server = Server::new(vec![]).await;
    let hold = Arc::new(Notify::new());
    server.state.lock().unwrap().hold = Some(hold.clone());
    let control = Control::fixture(false, true);
    let mut window = Window::new(control.epoch(), control.metadata(), chrono::Utc::now());
    window.add(Observation::Launch);
    window.add(Observation::Turn {
        result: TurnResult::Completed,
        mode: WorkMode::Code,
        subagent: false,
        error: None,
    });
    let mut pending = Some(Pending::from(window));
    let clear = Notify::new();
    let transport = Transport::at("synthetic-key", &server.endpoint).unwrap();
    tokio::select! {
        _ = deliver(&control, &mut pending, &transport, &clear, None) => panic!("request was not held"),
        _ = observed(&server, 1) => {}
    }
    assert_eq!(pending.as_ref().unwrap().batches.len(), 1);
    server.state.lock().unwrap().hold = None;
    hold.notify_waiters();
    deliver(
        &control,
        &mut pending,
        &transport,
        &clear,
        Some(Instant::now() + Duration::from_secs(1)),
    )
    .await;
    let state = server.state.lock().unwrap();
    assert_eq!(state.calls.len(), 2);
    assert_eq!(state.calls[0].1["table"], "app_launches");
    assert_eq!(state.calls[1].1["table"], "turns");
}

#[tokio::test(start_paused = true)]
async fn flushes_after_fifteen_minutes() {
    use super::super::{Control, Envelope, Observation, Shutdown, worker};
    use tokio::sync::{Notify, mpsc, oneshot};
    let server = Server::new(vec![]).await;
    let control = Control::fixture(false, true);
    let (records, received) = mpsc::channel(16);
    let (shutdown, closing) = mpsc::channel(1);
    records
        .send(Envelope {
            epoch: control.epoch(),
            observation: Observation::Launch,
        })
        .await
        .unwrap();
    let task = tokio::spawn(worker(
        control,
        received,
        closing,
        Arc::new(Notify::new()),
        Transport::at("synthetic-key", &server.endpoint).unwrap(),
    ));
    while records.capacity() != 16 {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_secs(14 * 60 + 59)).await;
    assert!(server.state.lock().unwrap().calls.is_empty());
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::time::resume();
    observed(&server, 1).await;
    let (reply, ack) = oneshot::channel();
    let deadline = Instant::now() + Duration::from_secs(1);
    shutdown.send(Shutdown { deadline, reply }).await.unwrap();
    ack.await.unwrap();
    task.await.unwrap();
    assert_eq!(server.state.lock().unwrap().calls.len(), 1);
}

#[tokio::test]
#[ignore = "requires SAILRY_LIVE_TELEMETRY_KEY and SAILRY_LIVE_TELEMETRY_ENDPOINT"]
async fn ingests_synthetic_metrics() {
    use super::super::{
        Model, Observation, RequestResult, StatusClass, Tokens, Tool, ToolResult, TurnResult,
        aggregate::Window,
    };
    use sailry_protocol::{WorkMode, search::Driver};
    assert!(
        crate::telemetry::Configuration::read().allowed,
        "live reporting is disabled"
    );
    let key = std::env::var("SAILRY_LIVE_TELEMETRY_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .expect("live telemetry key is required");
    let endpoint = std::env::var("SAILRY_LIVE_TELEMETRY_ENDPOINT")
        .expect("live telemetry endpoint is required");
    assert!(
        endpoint == ENDPOINT,
        "live telemetry endpoint must be the official Log API"
    );
    let metadata = crate::telemetry::Metadata {
        installation_id: uuid::Uuid::new_v4().to_string(),
        release: format!(
            "sailry@{}+{}",
            env!("SAILRY_HOST_VERSION"),
            env!("SAILRY_GIT_SHA")
        ),
        channel: "dev",
        application: "host",
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    };
    let mut window = Window::new(0, metadata, chrono::Utc::now());
    let provider = serde_json::from_value(json!({
        "id":sailry_protocol::ProviderId::new(),"revision":1,"name":"Synthetic provider",
        "api":"responses","authentication":"api_key","endpoint":"https://example.invalid",
        "enabled":true,"default_model":"synthetic","credential":null
    }))
    .unwrap();
    for observation in [
        Observation::Launch,
        Observation::Turn {
            result: TurnResult::Completed,
            mode: WorkMode::Code,
            subagent: false,
            error: None,
        },
        Observation::Tool {
            tool: Tool::builtin("read_session").unwrap(),
            result: ToolResult::Success,
            error: None,
            elapsed: Duration::from_millis(1),
        },
        Observation::Model {
            model: Model::capture(&provider, "synthetic"),
            result: RequestResult::Success,
            status: StatusClass::Success,
            error: None,
            retries: Some(0),
            tokens: Tokens::default(),
        },
        Observation::Search {
            driver: Some(Driver::Exa),
            result: RequestResult::Success,
            error: None,
            elapsed: Duration::from_millis(1),
        },
    ] {
        window.add(observation);
    }
    let batches = window.finish(chrono::Utc::now());
    assert_eq!(batches.len(), 5);
    let transport =
        Transport::at(&key, &endpoint).expect("live telemetry transport is unavailable");
    for batch in batches {
        assert!(
            transport
                .send(&batch, Some(Instant::now() + Duration::from_secs(10)))
                .await,
            "live telemetry ingestion did not return HTTP success"
        );
    }
}
