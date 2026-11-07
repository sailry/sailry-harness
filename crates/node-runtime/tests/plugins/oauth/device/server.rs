use super::*;
use sailry_link::CancellationToken;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Answers like GitHub: form-encoded bodies unless a request accepts JSON.
pub(super) struct Server {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<(String, String)>>>,
    pub refresh_error: Arc<Mutex<Option<(u16, &'static str)>>>,
    stop: CancellationToken,
}

impl Server {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/login/oauth/access_token",
            listener.local_addr().unwrap()
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let refresh_error = Arc::new(Mutex::new(None));
        let stop = CancellationToken::new();
        let (recorded, closed) = (requests.clone(), stop.clone());
        let rejected = refresh_error.clone();
        tokio::spawn(async move {
            let polls = Arc::new(AtomicUsize::new(0));
            let refreshes = Arc::new(AtomicUsize::new(0));
            let mut workers = tokio::task::JoinSet::new();
            loop {
                let mut stream = tokio::select! { _=closed.cancelled()=>break, result=listener.accept()=>result.unwrap().0 };
                let (recorded, polls, refreshes) =
                    (recorded.clone(), polls.clone(), refreshes.clone());
                let rejected = rejected.clone();
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
                    recorded.lock().unwrap().push((head.clone(), body.clone()));
                    let fields: BTreeMap<_, _> = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
                    let path = head.split_whitespace().nth(1).unwrap_or_default();
                    let (status, document) = match (path, fields.get("grant_type").map(String::as_str)) {
                        ("/login/device/code", _) => (200, json!({"device_code":"private-device-marker","user_code":"WDJB-MJHT",
                            "verification_uri":"https://issuer.example.com/login/device","expires_in":900,"interval":1})),
                        (_, Some("refresh_token")) => {
                            if let Some((status, error)) = *rejected.lock().unwrap() {
                                (status, json!({"error": error}))
                            } else {
                                let generation = refreshes.fetch_add(1, Ordering::SeqCst) + 1;
                                (200, tokens(generation, if generation < 2 { 1 } else { 3600 }))
                            }
                        }
                        _ if polls.fetch_add(1, Ordering::SeqCst) == 0 => (200, json!({"error":"authorization_pending"})),
                        _ => (200, tokens(0, 1)),
                    };
                    let accepted = head.lines().any(|line| line.split_once(':').is_some_and(|(name, value)| name.eq_ignore_ascii_case("accept") && value.trim() == "application/json"));
                    let (kind, body) = if accepted {
                        ("application/json", document.to_string())
                    } else {
                        let mut form = url::form_urlencoded::Serializer::new(String::new());
                        for (key, value) in document.as_object().unwrap() {
                            form.append_pair(key, &value.as_str().map_or_else(|| value.to_string(), str::to_owned));
                        }
                        ("application/x-www-form-urlencoded", form.finish())
                    };
                    let _ = stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
                });
            }
            workers.abort_all();
            while workers.join_next().await.is_some() {}
        });
        Self {
            endpoint,
            requests,
            refresh_error,
            stop,
        }
    }
}

fn tokens(generation: usize, expires_in: u64) -> serde_json::Value {
    json!({"access_token":format!("private-access-marker-{generation}"),
        "refresh_token":format!("private-refresh-marker-{generation}"),
        "expires_in":expires_in,"token_type":"bearer","scope":""})
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
