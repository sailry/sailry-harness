//! Test-only upstream budget and fixture scope gate, not another Agent runner.
use super::scope::Scope;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{
    Request, Response,
    body::{Bytes, Incoming},
    service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use sailry_protocol::Secret;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    convert::Infallible,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    net::TcpListener,
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;

const LIMIT: usize = 16 * 1024 * 1024;
pub(super) const CAP: usize = 10;
pub(super) const LOCAL_KEY: &str = "isolated-computer-acceptance";

#[derive(Default)]
struct Records {
    attempts: usize,
    refusal: Option<&'static str>,
    responses: Vec<Value>,
    previous_failures: Vec<Value>,
}

struct State {
    http: reqwest::Client,
    upstream: String,
    secret: Secret,
    scope: Scope,
    records: Mutex<Records>,
    journal: Option<PathBuf>,
}

pub(super) struct Relay {
    pub endpoint: String,
    state: Arc<State>,
    stop: CancellationToken,
    server: Option<JoinHandle<()>>,
}

impl Relay {
    pub async fn start(
        endpoint: &str,
        secret: Secret,
        scope: Scope,
        journal: Option<PathBuf>,
    ) -> Self {
        // The only real credential stays here in memory. The isolated Node gets
        // a dummy credential and a loopback endpoint, including in its history.
        assert!(!secret.expose().is_empty(), "upstream credential required");
        let upstream = format!("{}/responses", endpoint.trim_end_matches('/'));
        let http = reqwest::Client::builder()
            .retry(reqwest::retry::never())
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(180))
            .build()
            .expect("bounded acceptance HTTP client");
        let records =
            journal
                .as_ref()
                .filter(|path| path.exists())
                .map_or_else(Records::default, |path| {
                    Records::restore(
                        &serde_json::from_slice::<Value>(
                            &std::fs::read(path).expect("retained acceptance budget"),
                        )
                        .expect("valid retained acceptance budget"),
                    )
                    .expect("resumable acceptance budget")
                });
        let state = Arc::new(State {
            http,
            upstream,
            secret,
            scope,
            records: Mutex::new(records),
            journal,
        });
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback acceptance listener");
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        let shared = state.clone();
        let server = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    _ = stopped.cancelled() => break,
                    accepted = listener.accept(), if connections.len() < 4 => {
                        let Ok((socket, _)) = accepted else { break; };
                        let shared = shared.clone();
                        connections.spawn(async move {
                            let service = service_fn(move |request| {
                                let shared = shared.clone();
                                async move { Ok::<_, Infallible>(shared.respond(request).await) }
                            });
                            let mut builder = hyper::server::conn::http1::Builder::new();
                            builder.timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(5));
                            let _ = builder.serve_connection(TokioIo::new(socket), service).await;
                        });
                    },
                    _ = connections.join_next(), if !connections.is_empty() => {},
                }
            }
            connections.shutdown().await;
        });
        Self {
            endpoint,
            state,
            stop,
            server: Some(server),
        }
    }

    pub fn report(&self) -> Value {
        let records = self.state.records.lock().unwrap();
        records.report()
    }

    pub fn refused(&self) -> bool {
        self.state.records.lock().unwrap().refusal.is_some()
    }

    pub async fn close(mut self) -> Value {
        self.stop.cancel();
        if let Some(server) = self.server.take() {
            server.await.expect("acceptance relay shutdown");
        }
        self.report()
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl State {
    fn persist(&self, records: &Records) -> bool {
        self.journal.as_ref().is_none_or(|path| {
            let Some(parent) = path.parent() else {
                return false;
            };
            let Ok(mut file) = tempfile::NamedTempFile::new_in(parent) else {
                return false;
            };
            if file
                .write_all(&serde_json::to_vec_pretty(&records.report()).unwrap())
                .is_err()
            {
                return false;
            }
            file.persist(path).is_ok()
        })
    }

    fn fail(&self, reason: &'static str) -> Response<Full<Bytes>> {
        {
            let mut records = self.records.lock().unwrap();
            records.refusal.get_or_insert(reason);
            self.persist(&records);
        }
        // A non-retryable error avoids spending the SDK retry budget on a gate.
        Response::builder()
            .status(400)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(
                serde_json::to_vec(&json!({"error": {
                    "message": reason, "type": "invalid_request_error", "code": "acceptance_gate"
                }}))
                .unwrap(),
            )))
            .unwrap()
    }

    async fn respond(&self, request: Request<Incoming>) -> Response<Full<Bytes>> {
        if request.method() != hyper::Method::POST
            || request.uri().path() != "/v1/responses"
            || request.uri().query().is_some()
            || request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                != Some("Bearer isolated-computer-acceptance")
        {
            return self.fail("acceptance route or authentication refused");
        }
        if self.records.lock().unwrap().refusal.is_some() {
            return self.fail("acceptance gate is closed");
        }
        let bytes = match tokio::time::timeout(
            Duration::from_secs(10),
            Limited::new(request.into_body(), LIMIT).collect(),
        )
        .await
        {
            Ok(Ok(body)) => body.to_bytes(),
            _ => return self.fail("acceptance request body refused"),
        };
        let mut body: Value = match serde_json::from_slice(&bytes) {
            Ok(body) => body,
            Err(_) => return self.fail("acceptance request JSON refused"),
        };
        if let Err(reason) = self.scope.filter_request(&mut body) {
            return self.fail(reason);
        }
        let bytes = if self.scope.preserves_request() {
            bytes.to_vec()
        } else {
            serde_json::to_vec(&body).unwrap()
        };
        // Reserve directly before the sole send. Failed networks and SDK retries
        // consume attempts too; no request ever refunds or bypasses this count.
        {
            let mut records = self.records.lock().unwrap();
            if records.refusal.is_some() || records.attempts >= CAP {
                drop(records);
                return self.fail("acceptance model request cap reached");
            }
            records.attempts += 1;
            if !self.persist(&records) {
                drop(records);
                return self.fail("acceptance request journal failed");
            }
        }
        let started = Instant::now();
        let result = self
            .http
            .post(&self.upstream)
            .bearer_auth(self.secret.expose())
            .header("content-type", "application/json")
            .body(bytes.clone())
            .send()
            .await;
        let mut response = match result {
            Ok(response) => response,
            Err(_) => return self.fail("acceptance upstream request failed"),
        };
        let headers_ms = started.elapsed().as_millis();
        if !response.status().is_success() {
            return self.fail("acceptance upstream response failed");
        }
        let mut output = Vec::new();
        let mut first_byte_ms = None;
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    first_byte_ms.get_or_insert_with(|| started.elapsed().as_millis());
                    if chunk.len() > LIMIT - output.len() {
                        return self.fail("acceptance upstream body exceeded limit");
                    }
                    output.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(_) => return self.fail("acceptance upstream body failed"),
            }
        }
        if output
            .windows(self.secret.expose().len())
            .any(|window| window == self.secret.expose().as_bytes())
        {
            return self.fail("acceptance upstream credential echo refused");
        }
        // Buffer before any byte reaches ADK: native tool calls and text markup
        // must both be checked before the SDK can admit a side effect.
        let completed = match self.scope.validate_response(&output).await {
            Ok(completed) => completed,
            Err(reason) => return self.fail(reason),
        };
        if let Err(reason) = reject_decoded_echo(&output, self.secret.expose()).await {
            return self.fail(reason);
        }
        {
            let mut records = self.records.lock().unwrap();
            records.responses.push(json!({
                "request_bytes": bytes.len(), "response_bytes": output.len(),
                "headers_ms": headers_ms, "first_byte_ms": first_byte_ms,
                "elapsed_ms": started.elapsed().as_millis(), "usage": usage(&completed["usage"]),
            }));
            if !self.persist(&records) {
                drop(records);
                return self.fail("acceptance response journal failed");
            }
        }
        Response::builder()
            .header("content-type", "text/event-stream")
            .body(Full::new(Bytes::from(output)))
            .unwrap()
    }
}

impl Records {
    fn restore(value: &Value) -> Result<Self, &'static str> {
        let attempts = value["upstream_attempts"]
            .as_u64()
            .and_then(|attempts| usize::try_from(attempts).ok())
            .filter(|attempts| *attempts > 0 && *attempts < CAP)
            .ok_or("retained acceptance request count is invalid or exhausted")?;
        if value["cap"].as_u64() != Some(CAP as u64) {
            return Err("retained acceptance cap changed");
        }
        let reason = value["refusal"]
            .as_str()
            .filter(|reason| {
                matches!(
                    *reason,
                    "model response did not complete safely"
                        | "acceptance upstream request failed"
                        | "acceptance upstream response failed"
                        | "acceptance upstream body failed"
                )
            })
            .ok_or("only a withheld model failure can resume")?;
        let responses = value["responses"]
            .as_array()
            .filter(|responses| responses.len() <= attempts)
            .ok_or("retained acceptance usage records are invalid")?
            .clone();
        let mut previous_failures = match value.get("previous_failures") {
            None => Vec::new(),
            Some(value) => value
                .as_array()
                .filter(|failures| failures.is_empty())
                .ok_or("retained acceptance failures are invalid")?
                .clone(),
        };
        previous_failures.push(json!({"upstream_attempts":attempts,"reason":reason}));
        Ok(Self {
            attempts,
            refusal: None,
            responses,
            previous_failures,
        })
    }

    fn report(&self) -> Value {
        json!({"cap": CAP, "upstream_attempts": self.attempts,
            "refusal": self.refusal, "responses": self.responses,
            "previous_failures": self.previous_failures,
            "usage_coverage_complete": self.responses.len() == self.attempts
                && self.responses.iter().all(|response| !response["usage"].is_null())})
    }
}

fn usage(value: &Value) -> Value {
    let Some(input) = value["input_tokens"].as_u64() else {
        return Value::Null;
    };
    let Some(output) = value["output_tokens"].as_u64() else {
        return Value::Null;
    };
    json!({"input_tokens": input, "output_tokens": output,
        "cached_input_tokens": value["input_tokens_details"]["cached_tokens"].as_u64(),
        "reasoning_tokens": value["output_tokens_details"]["reasoning_tokens"].as_u64()})
}

async fn reject_decoded_echo(bytes: &[u8], secret: &str) -> Result<(), &'static str> {
    let mut events = futures::stream::iter([Ok::<_, Infallible>(bytes)]).eventsource();
    let mut tails: BTreeMap<(String, u64, String), Vec<u8>> = BTreeMap::new();
    let mut emitted_text = Vec::new();
    while let Some(event) = events.next().await {
        let event = event.map_err(|_| "acceptance upstream SSE refused")?;
        if event.event == "keepalive" || event.data.trim().is_empty() || event.data == "[DONE]" {
            continue;
        }
        let value: Value =
            serde_json::from_str(&event.data).map_err(|_| "acceptance upstream JSON refused")?;
        if contains_echo(&value, secret) {
            return Err("acceptance upstream credential echo refused");
        }
        if let Some(delta) = value["delta"].as_str() {
            // ADK emits text deltas in arrival order without item/content indices.
            if value["type"] == "response.output_text.delta" {
                check_delta(&mut emitted_text, delta, secret)?;
            }
            let key = (
                value["item_id"].as_str().unwrap_or("").to_owned(),
                value["content_index"]
                    .as_u64()
                    .or_else(|| value["summary_index"].as_u64())
                    .unwrap_or(0),
                value["type"].as_str().unwrap_or("").to_owned(),
            );
            let tail = tails.entry(key).or_default();
            check_delta(tail, delta, secret)?;
        }
    }
    Ok(())
}

fn check_delta(tail: &mut Vec<u8>, delta: &str, secret: &str) -> Result<(), &'static str> {
    let mut combined = std::mem::take(tail);
    combined.extend_from_slice(delta.as_bytes());
    if combined
        .windows(secret.len())
        .any(|window| window == secret.as_bytes())
    {
        return Err("acceptance upstream credential echo refused");
    }
    *tail = combined[combined.len().saturating_sub(secret.len() - 1)..].to_vec();
    Ok(())
}

fn contains_echo(value: &Value, secret: &str) -> bool {
    match value {
        Value::String(value) => {
            value.contains(secret)
            // Function arguments are a JSON string and ADK decodes them again.
            || serde_json::from_str::<Value>(value).is_ok_and(|value| contains_echo(&value, secret))
        }
        Value::Array(values) => values.iter().any(|value| contains_echo(value, secret)),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| key.contains(secret) || contains_echo(value, secret)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn upstream(status: u16) -> (String, Arc<AtomicUsize>, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let server = tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let count = count.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request: Request<Incoming>| {
                        let count = count.clone();
                        async move {
                            assert_eq!(
                                request.headers()["authorization"],
                                "Bearer mock-upstream-secret"
                            );
                            let body: Value = serde_json::from_slice(
                                &request.into_body().collect().await.unwrap().to_bytes(),
                            )
                            .unwrap();
                            assert!(body["tools"].as_array().unwrap().is_empty());
                            count.fetch_add(1, Ordering::SeqCst);
                            let terminal = json!({"type":"response.completed", "response": {
                                "id":"resp_mock", "status":"completed", "output":[],
                                "usage":{"input_tokens":12,"output_tokens":3}
                            }});
                            let text = format!(
                                "event: response.completed\ndata: {terminal}\n\ndata: [DONE]\n\n"
                            );
                            Ok::<_, Infallible>(
                                Response::builder()
                                    .status(status)
                                    .body(Full::new(Bytes::from(text)))
                                    .unwrap(),
                            )
                        }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(socket), service)
                        .await;
                });
            }
        });
        (endpoint, calls, server)
    }

    async fn call(relay: &Relay, model: &str) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!("{}/responses", relay.endpoint))
            .bearer_auth(LOCAL_KEY)
            .json(&json!({"model":model,"stream":true,"tools":[],"input":[]}))
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn caps_actual_sends() {
        let (endpoint, calls, server) = upstream(200).await;
        let directory = tempfile::tempdir().unwrap();
        let journal = directory.path().join("requests.json");
        let relay = Relay::start(
            &endpoint,
            Secret::new("mock-upstream-secret".into()),
            Scope::new(1, 2, "gpt-6-luna".into()),
            Some(journal.clone()),
        )
        .await;
        for _ in 0..CAP {
            assert_eq!(call(&relay, "gpt-6-luna").await.status(), 200);
        }
        assert_eq!(call(&relay, "gpt-6-luna").await.status(), 400);
        assert_eq!(calls.load(Ordering::SeqCst), CAP);
        assert_eq!(relay.report()["upstream_attempts"], CAP);
        assert_eq!(relay.report()["responses"].as_array().unwrap().len(), CAP);
        let saved: Value = serde_json::from_slice(&std::fs::read(journal).unwrap()).unwrap();
        assert_eq!(saved["upstream_attempts"], CAP);
        assert!(!saved.to_string().contains("mock-upstream-secret"));
        relay.close().await;
        server.abort();
    }

    #[tokio::test]
    async fn refuses_model_before_sending() {
        let (endpoint, calls, server) = upstream(200).await;
        let relay = Relay::start(
            &endpoint,
            Secret::new("mock-upstream-secret".into()),
            Scope::new(1, 2, "gpt-6-luna".into()),
            None,
        )
        .await;
        assert_eq!(call(&relay, "different-model").await.status(), 400);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        relay.close().await;
        server.abort();
    }

    #[tokio::test]
    async fn does_not_retry_upstream_errors() {
        let (endpoint, calls, server) = upstream(503).await;
        let relay = Relay::start(
            &endpoint,
            Secret::new("mock-upstream-secret".into()),
            Scope::new(1, 2, "gpt-6-luna".into()),
            None,
        )
        .await;
        assert_eq!(call(&relay, "gpt-6-luna").await.status(), 400);
        assert_eq!(call(&relay, "gpt-6-luna").await.status(), 400);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        relay.close().await;
        server.abort();
    }

    #[tokio::test]
    async fn resumes_without_refunding_or_resetting_the_cap() {
        let directory = tempfile::tempdir().unwrap();
        let journal = directory.path().join("requests.json");
        let (failed_endpoint, failures, failed_server) = upstream(503).await;
        let relay = Relay::start(
            &failed_endpoint,
            Secret::new("mock-upstream-secret".into()),
            Scope::new(1, 2, "gpt-6-luna".into()),
            Some(journal.clone()),
        )
        .await;
        assert_eq!(call(&relay, "gpt-6-luna").await.status(), 400);
        assert_eq!(relay.close().await["upstream_attempts"], 1);
        let (endpoint, calls, server) = upstream(200).await;
        let relay = Relay::start(
            &endpoint,
            Secret::new("mock-upstream-secret".into()),
            Scope::new(1, 2, "gpt-6-luna".into()),
            Some(journal),
        )
        .await;
        assert_eq!(relay.report()["upstream_attempts"], 1);
        for _ in 1..CAP {
            assert_eq!(call(&relay, "gpt-6-luna").await.status(), 200);
        }
        assert_eq!(call(&relay, "gpt-6-luna").await.status(), 400);
        assert_eq!(
            failures.load(Ordering::SeqCst) + calls.load(Ordering::SeqCst),
            CAP
        );
        let report = relay.close().await;
        assert_eq!(report["upstream_attempts"], CAP);
        assert_eq!(report["previous_failures"][0]["upstream_attempts"], 1);
        assert_eq!(report["usage_coverage_complete"], false);
        failed_server.abort();
        server.abort();
    }

    #[test]
    fn refuses_scope_failures_and_exhausted_budgets() {
        let mut value = json!({"cap":CAP,"upstream_attempts":1,"responses":[],
            "refusal":"model function is outside the permitted scope"});
        assert!(Records::restore(&value).is_err());
        value["refusal"] = json!("model response did not complete safely");
        assert!(Records::restore(&value).is_ok());
        value["previous_failures"] = json!([{"upstream_attempts":1}]);
        assert!(Records::restore(&value).is_err());
        value["previous_failures"] = json!([]);
        value["upstream_attempts"] = json!(CAP);
        assert!(Records::restore(&value).is_err());
    }

    #[tokio::test]
    async fn rejects_decoded_and_split_credential_echoes() {
        let secret = "mock-upstream-secret";
        let escaped =
            br#"data: {"type":"response.output_text.delta","delta":"\u006dock-upstream-secret"}

"#;
        assert!(reject_decoded_echo(escaped, secret).await.is_err());
        let arguments = json!({"type":"response.completed", "response":{"output":[{
            "type":"function_call", "arguments":r#"{"text":"\u006dock-upstream-secret"}"#
        }]}});
        assert!(
            reject_decoded_echo(format!("data: {arguments}\n\n").as_bytes(), secret)
                .await
                .is_err()
        );
        let split = ["mock-upstream-", "secret"].into_iter().map(|delta| {
            format!("data: {}\n\n", json!({"type":"response.output_text.delta", "item_id":"message", "content_index":0,"delta":delta}))
        }).collect::<String>();
        assert!(reject_decoded_echo(split.as_bytes(), secret).await.is_err());
        for (item, index) in [("message", 1), ("another-message", 0)] {
            let split = format!(
                "data: {{\"type\":\"response.output_text.delta\",\"item_id\":\"message\",\"content_index\":0,\"delta\":\"\\u006dock-upstream-\"}}\n\ndata: {}\n\n",
                json!({"type":"response.output_text.delta", "item_id":item, "content_index":index,"delta":"secret"})
            );
            assert!(!split.contains(secret));
            assert!(reject_decoded_echo(split.as_bytes(), secret).await.is_err());
        }
        assert!(
            reject_decoded_echo(
                b"event: keepalive\ndata: ping\n\ndata: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n",
                secret
            )
            .await
            .is_ok()
        );
    }
}
