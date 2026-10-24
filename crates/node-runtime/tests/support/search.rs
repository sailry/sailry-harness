use sailry_link::CancellationToken;
use sailry_protocol::search::Driver;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex},
};
use tokio::{io::AsyncReadExt, net::TcpListener};

#[path = "search/formatted.rs"]
mod formatted;
#[path = "search/transport.rs"]
mod transport;

pub enum Reply {
    Status(u16, Option<String>),
    Json(Value),
}

pub struct Server {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<(String, Value)>>>,
    replies: Arc<Mutex<BTreeMap<String, VecDeque<Reply>>>>,
    pub releases: Arc<Mutex<BTreeMap<String, VecDeque<CancellationToken>>>>,
    pub holds: Arc<Mutex<BTreeSet<String>>>,
    pub(super) stalls: Arc<Mutex<BTreeSet<String>>>,
    pub held: CancellationToken,
    pub disconnected: CancellationToken,
    stop: CancellationToken,
}

impl Server {
    pub async fn start() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let replies = Arc::new(Mutex::new(BTreeMap::<String, VecDeque<Reply>>::new()));
        let stop = CancellationToken::new();
        let held = CancellationToken::new();
        let disconnected = CancellationToken::new();
        let server = Self {
            endpoint,
            requests: requests.clone(),
            replies: replies.clone(),
            releases: Default::default(),
            holds: Default::default(),
            stalls: Default::default(),
            stop: stop.clone(),
            held: held.clone(),
            disconnected: disconnected.clone(),
        };
        let releases = server.releases.clone();
        let holds = server.holds.clone();
        let stalls = server.stalls.clone();
        let held = server.held.clone();
        let disconnected = server.disconnected.clone();
        tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                let (mut stream, _) = tokio::select! { _=stop.cancelled()=>break, result=listener.accept()=>result.unwrap() };
                let (requests, replies, releases, holds, stalls, held, disconnected) = (
                    requests.clone(),
                    replies.clone(),
                    releases.clone(),
                    holds.clone(),
                    stalls.clone(),
                    held.clone(),
                    disconnected.clone(),
                );
                connections.spawn(async move {
                    let Some(request) = transport::read(&mut stream).await else {
                        return;
                    };
                    let transport::Request {
                        header,
                        body,
                        method,
                        path,
                    } = request;
                    requests.lock().unwrap().push((header, body.clone()));
                    let stage = body.get("method").and_then(Value::as_str);
                    let holding = holds.lock().unwrap().remove(&path)
                        || stage.is_some_and(|stage| stalls.lock().unwrap().remove(stage));
                    if holding {
                        held.cancel();
                        let _ = stream.read(&mut [0; 1]).await;
                        disconnected.cancel();
                        return;
                    }
                    let override_reply = replies
                        .lock()
                        .unwrap()
                        .get_mut(&path)
                        .and_then(VecDeque::pop_front);
                    let release = releases
                        .lock()
                        .unwrap()
                        .get_mut(&path)
                        .and_then(VecDeque::pop_front);
                    let (status, retry, body) = match (release, override_reply) {
                        (Some(release), _) => {
                            held.cancel();
                            let mut buffer = [0; 1];
                            tokio::select! {
                                _ = release.cancelled() => {},
                                _ = stream.read(&mut buffer) => { disconnected.cancel(); return; },
                            }
                            (200, None, mapping(&path).to_string())
                        }
                        (None, Some(Reply::Status(status, retry))) => (
                            status,
                            retry,
                            json!({"error":"private-response"}).to_string(),
                        ),
                        (None, Some(Reply::Json(body))) => (200, None, body.to_string()),
                        (None, None) if path == "/mcp" => {
                            let (status, body) = formatted::response(&method, &body, false);
                            (status, None, body)
                        }
                        (None, None) if path == "/modern-mcp" => {
                            let (status, body) = formatted::response(&method, &body, true);
                            (status, None, body)
                        }
                        (None, None) => (200, None, mapping(&path).to_string()),
                    };
                    let session =
                        (stage == Some("initialize") && status == 200).then_some("search-fixture");
                    transport::respond(&mut stream, status, retry.as_deref(), &body, session).await;
                });
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });
        server
    }

    pub fn url(&self, driver: Driver) -> String {
        format!(
            "{}/{}",
            self.endpoint,
            match driver {
                Driver::Exa => "exa",
                Driver::Tavily => "tavily",
                Driver::Brave => "brave",
                Driver::Searxng => "searxng",
                Driver::Bocha => "bocha",
            }
        )
    }
    pub fn respond(&self, path: &str, replies: Vec<Reply>) {
        self.replies
            .lock()
            .unwrap()
            .insert(path.into(), replies.into());
    }
    pub fn count(&self, path: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(header, _)| {
                header
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .split('?')
                    .next()
                    == Some(path)
            })
            .count()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub fn mapping(path: &str) -> Value {
    let common = json!({"title":"Public source","url":"https://example.com/source","text":"Untrusted content","content":"Untrusted content","description":"Untrusted content","snippet":"Untrusted content","name":"Public source","publishedDate":"2026-10-09","published_date":"2026-10-09","page_age":"2026-10-09","datePublished":"2026-10-09"});
    match path {
        "/brave" => json!({"web":{"results":[common]}}),
        "/bocha" => json!({"code":200,"data":{"webPages":{"value":[common]}}}),
        _ => json!({"results":[common]}),
    }
}
