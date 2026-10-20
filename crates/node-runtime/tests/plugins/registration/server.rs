//! Isolated registration responses never contact the account provider.
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) struct Server {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<(Instant, String)>>>,
    pub replies: Arc<Mutex<VecDeque<Value>>>,
    stop: CancellationToken,
}

impl Server {
    pub async fn start(replies: Vec<Value>) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/registration", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let replies = Arc::new(Mutex::new(VecDeque::from(replies)));
        let stop = CancellationToken::new();
        let (recorded, responses, closed) = (requests.clone(), replies.clone(), stop.clone());
        tokio::spawn(async move {
            loop {
                let mut socket = tokio::select! {
                    _ = closed.cancelled() => break,
                    accepted = listener.accept() => accepted.unwrap().0,
                };
                let request = async {
                    let mut bytes = Vec::new();
                    let mut byte = [0];
                    while !bytes.ends_with(b"\r\n\r\n") {
                        socket.read_exact(&mut byte).await.unwrap();
                        bytes.push(byte[0]);
                        assert!(bytes.len() < 32 * 1024);
                    }
                    let header = String::from_utf8(bytes).unwrap();
                    assert!(header.starts_with("POST "));
                    let size: usize = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(str::to_owned)
                        })
                        .unwrap()
                        .parse()
                        .unwrap();
                    assert!(size < 32 * 1024);
                    let mut bytes = vec![0; size];
                    socket.read_exact(&mut bytes).await.unwrap();
                    let body = String::from_utf8(bytes).unwrap();
                    recorded
                        .lock()
                        .unwrap()
                        .push((Instant::now(), body.clone()));
                    if header.starts_with("POST /open-apis/auth/") {
                        let response =
                            json!({"tenant_access_token":"private-tenant-marker","expire":7200})
                                .to_string();
                        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).as_bytes()).await.unwrap();
                        return;
                    }
                    let fields: std::collections::BTreeMap<_, _> =
                        url::form_urlencoded::parse(body.as_bytes())
                            .into_owned()
                            .collect();
                    let response = match fields["action"].as_str() {
                        "init" => json!({"supported_auth_methods":["client_secret"]}),
                        "begin" => {
                            assert_eq!(fields["archetype"], "PersonalAgent");
                            assert_eq!(fields["auth_method"], "client_secret");
                            assert_eq!(fields["request_user_info"], "open_id");
                            json!({"device_code":"private-device-marker","interval":1,"expires_in":30,
                                "verification_uri_complete":"https://accounts.feishu.cn/registration?user_code=single-code&addons=broader-preset"})
                        },
                        "poll" => {
                            assert_eq!(fields["device_code"], "private-device-marker");
                            responses.lock().unwrap().pop_front().unwrap_or_else(|| json!({"error":"authorization_pending"}))
                        },
                        _ => panic!("unexpected registration action"),
                    }.to_string();
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
                };
                tokio::select! { _ = closed.cancelled() => break, _ = request => {} }
            }
        });
        Self {
            endpoint,
            requests,
            replies,
            stop,
        }
    }

    pub fn success() -> Value {
        json!({"client_id":"cli_registered","client_secret":"private-app-secret-marker","user_info":{"tenant_brand":"feishu","open_id":"unused-private-user-marker"}})
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
