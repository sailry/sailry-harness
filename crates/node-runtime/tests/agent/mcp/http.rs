use super::*;
mod bodies;
mod configuration;
use sailry_link::CancellationToken;
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinSet,
};

#[derive(Clone)]
enum Reply {
    Normal,
    Expired,
    Redirect(String),
    Oversized { method: &'static str, chunked: bool },
}

#[tokio::test]
async fn confines_private_headers() {
    for remote in [false, true] {
        for redirect in [false, true] {
            let target = Http::start(Reply::Normal).await;
            let origin = Http::start(if redirect {
                Reply::Redirect(target.endpoint.clone())
            } else {
                Reply::Normal
            })
            .await;
            let model = Server::tools(if redirect {
                vec![("load_skill".into(), json!({"skill":"example:analysis"}))]
            } else {
                vec![(alias("remote", "read"), json!({}))]
            })
            .await;
            let fixture = process::Fixture::new(remote, &model).await;
            package(
                &fixture.root,
                json!({"remote": {
                    "type":"streamable-http", "url":origin.endpoint, "headers":{"Authorization":""}
                }}),
                "private headers",
            );
            let root = fixture.root.join("package");
            let mut manifest: Value =
                serde_json::from_slice(&fs::read(root.join("plugin.json")).unwrap()).unwrap();
            manifest["extensions"] = json!({"dev.sailry.platform":{
                "api_version":"v1", "actions":[], "settings_schema":"dev.sailry.platform/settings.json"
            }});
            fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
            fs::write(root.join("dev.sailry.platform/settings.json"), json!({
                "$schema":plugin::settings::SCHEMA, "type":"object", "additionalProperties":false,
                "properties":{"token":{"type":"string", "x-sailry-secret":{"server":"remote", "header":"authorization"}}},
                "required":["token"]
            }).to_string()).unwrap();
            let package = install(&fixture, 0).await;
            let invalid = fixture.client.prepare(Command::SavePluginSettings {
                package: package.summary.reference(),
                values: BTreeMap::new(),
                secrets: BTreeMap::from([(
                    "token".into(),
                    plugin::settings::SecretUpdate::Replace(Secret::new(
                        "invalid\r\nheader".into(),
                    )),
                )]),
            });
            assert_eq!(
                fixture.client.execute(invalid).await.unwrap_err().code,
                ErrorCode::InvalidRequest
            );
            let secret = "Bearer private-fixture-${PLUGIN_ROOT}";
            let settings = execute(
                &fixture.client,
                Command::SavePluginSettings {
                    package: package.summary.reference(),
                    values: BTreeMap::new(),
                    secrets: BTreeMap::from([(
                        "token".into(),
                        plugin::settings::SecretUpdate::Replace(Secret::new(secret.into())),
                    )]),
                },
            )
            .await;
            assert!(!serde_json::to_string(&settings).unwrap().contains(secret));

            let (_, turn) = submit(&fixture, false).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            assert!(!serde_json::to_string(&page).unwrap().contains(secret));
            assert!(target.requests.lock().unwrap().is_empty());
            assert_eq!(origin.count("initialize"), 1);
            assert_eq!(origin.count("tools/call"), usize::from(!redirect));
            for (_, headers, _) in origin.requests.lock().unwrap().iter() {
                assert_eq!(
                    headers.get("authorization").map(String::as_str),
                    Some(secret)
                );
            }
            assert!(
                !serde_json::to_string(&*model.requests.lock().unwrap())
                    .unwrap()
                    .contains(secret)
            );
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

struct Http {
    endpoint: String,
    requests: Records,
    effects: Arc<AtomicUsize>,
    stop: CancellationToken,
}

type Records = Arc<Mutex<Vec<(String, BTreeMap<String, String>, Value)>>>;

impl Drop for Http {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Http {
    async fn start(reply: Reply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let effects = Arc::new(AtomicUsize::new(0));
        let stop = CancellationToken::new();
        let (records, writes, stopped) = (requests.clone(), effects.clone(), stop.clone());
        tokio::spawn(async move {
            let mut work = JoinSet::new();
            loop {
                tokio::select! {
                    _ = stopped.cancelled() => break,
                    Some(_) = work.join_next(), if !work.is_empty() => {},
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        let (records, writes, reply) = (records.clone(), writes.clone(), reply.clone());
                        work.spawn(async move { let _ = respond(stream, records, writes, reply).await; });
                    }
                }
            }
            work.shutdown().await;
        });
        Self {
            endpoint,
            requests,
            effects,
            stop,
        }
    }
    fn count(&self, method: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, _, value)| value["method"] == method)
            .count()
    }
    fn deletes(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, _, _)| method == "DELETE")
            .count()
    }
}

async fn respond(
    mut stream: TcpStream,
    records: Records,
    effects: Arc<AtomicUsize>,
    reply: Reply,
) -> std::io::Result<()> {
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
    let request = if length == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&bytes[end..end + length]).unwrap()
    };
    records
        .lock()
        .unwrap()
        .push((method.clone(), headers, request.clone()));
    if let Reply::Oversized { method, chunked } = reply
        && request["method"] == method
    {
        if method == "tools/call" {
            effects.fetch_add(1, Ordering::SeqCst);
        }
        return bodies::oversized(&mut stream, chunked).await;
    }
    let mut extra = String::new();
    let (status, result) = match reply {
        Reply::Redirect(target) => {
            extra = format!("Location: {target}\r\n");
            (307, None)
        }
        _ if method == "GET" => (405, None),
        _ if method == "DELETE" => (204, None),
        _ => match request["method"].as_str().unwrap_or("") {
            "initialize" => {
                extra = "Mcp-Session-Id: fixture-session\r\n".into();
                (
                    200,
                    Some(
                        json!({"protocolVersion": request["params"]["protocolVersion"], "capabilities": {"tools": {}}, "serverInfo": {"name": "http-fixture", "version": "1"}}),
                    ),
                )
            }
            "tools/list" => (200, Some(peer::listed(&request["params"]["cursor"]))),
            "tools/call" => {
                effects.fetch_add(1, Ordering::SeqCst);
                if matches!(reply, Reply::Expired) {
                    (404, None)
                } else {
                    (
                        200,
                        Some(
                            json!({"content": [{"type": "text", "text": "HTTP full result 中文 🙂"}], "structuredContent": {"value": request["params"]["arguments"]["value"]}}),
                        ),
                    )
                }
            }
            _ => (202, None),
        },
    };
    let body = result
        .map(|result| json!({"jsonrpc": "2.0", "id": request["id"], "result": result}).to_string())
        .unwrap_or_default();
    stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}", body.len()).as_bytes()).await
}

#[tokio::test]
async fn expired_calls_prevent_replay() {
    for remote in [false, true] {
        for expired in [false, true] {
            let http = Http::start(if expired {
                Reply::Expired
            } else {
                Reply::Normal
            })
            .await;
            let content = "完整HTTP🙂".repeat(1200);
            let model =
                Server::tools(vec![(alias("remote", "write"), json!({"value": content}))]).await;
            let fixture = process::Fixture::new(remote, &model).await;
            package(
                &fixture.root,
                json!({"remote": {"type": "streamable-http", "url": http.endpoint, "headers": {"X-Public": "${PLUGIN_ROOT}", "Mcp-Protocol-Version": "invalid-package-override"}}}),
                "version",
            );
            install(&fixture, 0).await;

            let (_, turn) = submit(&fixture, false).await;
            let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
            assert_eq!(http.effects.load(Ordering::SeqCst), 0);
            fixture
                .controller
                .handle()
                .disconnect(fixture.node.id())
                .await;
            process::decide(&fixture.client, &approval, Decision::Approve).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
            let output = results(&page);
            if expired {
                assert_eq!(output[0]["error"]["code"], "outcome_unknown");
            } else {
                assert_eq!(output[0]["output"]["value"], content);
            }
            assert_eq!(http.count("initialize"), 1);
            assert_eq!(http.count("tools/list"), 2);
            assert_eq!(http.count("tools/call"), 1);
            assert_eq!(http.effects.load(Ordering::SeqCst), 1);
            assert_eq!(http.deletes(), usize::from(expired));
            for (_, headers, _) in http.requests.lock().unwrap().iter() {
                assert_eq!(
                    headers.get("x-public").map(String::as_str),
                    Some("${PLUGIN_ROOT}")
                );
                assert_ne!(
                    headers.get("mcp-protocol-version").map(String::as_str),
                    Some("invalid-package-override")
                );
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
            assert_eq!(http.deletes(), 1);
        }
    }
}

#[tokio::test]
async fn isolates_redirect_failures() {
    for remote in [false, true] {
        let target = Http::start(Reply::Normal).await;
        let origin = Http::start(Reply::Redirect(target.endpoint.clone())).await;
        let model = Server::tools(vec![(
            "load_skill".into(),
            json!({"skill": "example:analysis"}),
        )])
        .await;
        let fixture = process::Fixture::new(remote, &model).await;
        package(
            &fixture.root,
            json!({"redirect": {"type": "streamable-http", "url": origin.endpoint, "headers": {"X-Public": "origin-only"}}, "bad-handshake": peer::config("mcp::peer::stdio_peer", "bad_handshake")}),
            "version",
        );
        install(&fixture, 0).await;

        let (_, turn) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert!(
            results(&page)[0]["content"]
                .as_str()
                .unwrap()
                .contains("Complete skill beside MCP")
        );
        assert!(target.requests.lock().unwrap().is_empty());
        assert_eq!(origin.count("initialize"), 1);
        reaped(&fixture.node.profile().join("plugins/data/example"));
        let instructions = model.requests.lock().unwrap()[0]["messages"].to_string();
        assert!(instructions.contains("MCP server example/redirect is unavailable"));
        assert!(!instructions.contains("origin-only"));
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
