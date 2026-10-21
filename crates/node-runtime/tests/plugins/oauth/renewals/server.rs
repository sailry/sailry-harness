use super::*;
use sailry_link::CancellationToken;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::oneshot,
};

struct Gate {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

pub(super) struct Server {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<(String, String)>>>,
    pub expected: Arc<Mutex<String>>,
    pub denied: Arc<AtomicBool>,
    held: Arc<Mutex<Option<Gate>>>,
    stop: CancellationToken,
}
impl Server {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/token", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let expected = Arc::new(Mutex::new("account-a-access".to_owned()));
        let denied = Arc::new(AtomicBool::new(false));
        let held = Arc::new(Mutex::new(None::<Gate>));
        let stop = CancellationToken::new();
        let result = Self {
            endpoint,
            requests: requests.clone(),
            expected: expected.clone(),
            denied: denied.clone(),
            held: held.clone(),
            stop: stop.clone(),
        };
        tokio::spawn(async move {
            let mut workers = tokio::task::JoinSet::new();
            let renewals = Arc::new(AtomicUsize::new(0));
            loop {
                let mut stream = tokio::select! { _=stop.cancelled()=>break, result=listener.accept()=>result.unwrap().0 };
                let (requests, expected, denied, held, renewals) = (
                    requests.clone(),
                    expected.clone(),
                    denied.clone(),
                    held.clone(),
                    renewals.clone(),
                );
                workers.spawn(async move {
                    let mut bytes = Vec::new();
                    let mut byte = [0];
                    while !bytes.ends_with(b"\r\n\r\n") {
                        if stream.read_exact(&mut byte).await.is_err() { return; }
                        bytes.push(byte[0]);
                        assert!(bytes.len() < 32 * 1024);
                    }
                    let head = String::from_utf8(bytes).unwrap();
                    let length = head.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length: ").map(|length| length.parse::<usize>().unwrap())).unwrap_or(0);
                    let mut bytes = vec![0; length];
                    if stream.read_exact(&mut bytes).await.is_err() { return; }
                    let body = String::from_utf8(bytes).unwrap();
                    requests.lock().unwrap().push((head.clone(), body.clone()));
                    let (status, body) = if head.starts_with("POST /token ") {
                        let fields: BTreeMap<_, _> = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
                        let (access, refresh) = if fields["grant_type"] == "refresh_token" {
                            let gate = held.lock().unwrap().take();
                            if let Some(gate) = gate {
                                let _ = gate.entered.send(());
                                let _ = gate.release.await;
                            }
                            let revision = renewals.fetch_add(1, Ordering::SeqCst) + 1;
                            (format!("account-a-renewed-{revision}"), format!("account-a-refresh-{revision}"))
                        } else {
                            assert_eq!(fields["grant_type"], "authorization_code");
                            let account = if fields["code"] == "account-b-code" { "b" } else { "a" };
                            (format!("account-{account}-access"), format!("account-{account}-refresh"))
                        };
                        *expected.lock().unwrap() = access.clone();
                        (200, json!({"access_token":access,"refresh_token":refresh,"expires_in":3600,"token_type":"Bearer"}).to_string())
                    } else {
                        assert!(head.starts_with("GET /health "));
                        let accepted = !denied.load(Ordering::SeqCst) && head.lines().any(|line| {
                            line.split_once(':').is_some_and(|(name, value)| name.eq_ignore_ascii_case("authorization") && value.trim() == format!("Bearer {}", expected.lock().unwrap()))
                        });
                        (if accepted { 200 } else { 401 }, "{}".into())
                    };
                    let _ = stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await;
                });
            }
            workers.abort_all();
            while workers.join_next().await.is_some() {}
        });
        result
    }
    pub fn hold(&self) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (entered, observation) = oneshot::channel();
        let (release, completion) = oneshot::channel();
        assert!(
            self.held
                .lock()
                .unwrap()
                .replace(Gate {
                    entered,
                    release: completion
                })
                .is_none()
        );
        (observation, release)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
