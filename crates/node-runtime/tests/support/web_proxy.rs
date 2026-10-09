use sailry_link::CancellationToken;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub struct Proxy {
    pub url: String,
    pub requests: Arc<Mutex<Vec<String>>>,
    stop: CancellationToken,
}

pub async fn header(stream: &mut (impl AsyncRead + Unpin)) -> String {
    let mut bytes = Vec::new();
    loop {
        let byte = stream.read_u8().await.unwrap();
        bytes.push(byte);
        assert!(bytes.len() <= 8192, "fixture headers are bounded");
        if bytes.ends_with(b"\r\n\r\n") {
            return String::from_utf8(bytes).unwrap();
        }
    }
}

impl Proxy {
    pub async fn start(targets: Vec<SocketAddr>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = CancellationToken::new();
        let proxy = Self {
            url,
            requests: requests.clone(),
            stop: stop.clone(),
        };
        tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                let (mut stream, _) = tokio::select! {
                    _ = stop.cancelled() => break,
                    result = listener.accept() => result.unwrap(),
                };
                let targets = targets.clone();
                let requests = requests.clone();
                connections.spawn(async move {
                    let request = header(&mut stream).await;
                    let mut line = request.lines().next().unwrap().split_whitespace();
                    let method = line.next().unwrap();
                    let destination = line.next().unwrap();
                    assert_eq!(line.next(), Some("HTTP/1.1"));
                    let destination_url = if method == "CONNECT" {
                        reqwest::Url::parse(&format!("http://{destination}")).unwrap()
                    } else {
                        reqwest::Url::parse(destination).unwrap()
                    };
                    assert!(
                        destination_url.host_str().unwrap().ends_with(".test"),
                        "proxy must resolve the original test hostname"
                    );
                    let target = *targets
                        .iter()
                        .find(|address| {
                            Some(address.port()) == destination_url.port_or_known_default()
                        })
                        .expect("proxy may only resolve declared isolated targets");
                    let forwarded = if method == "CONNECT" {
                        None
                    } else {
                        assert_eq!(method, "GET");
                        let url = reqwest::Url::parse(destination).unwrap();
                        assert_eq!(url.scheme(), "http");
                        let mut forwarded = format!(
                            "GET {}{} HTTP/1.1\r\n",
                            url.path(),
                            url.query()
                                .map(|query| format!("?{query}"))
                                .unwrap_or_default()
                        );
                        for header in request.lines().skip(1).filter(|line| !line.is_empty()) {
                            let (name, _) = header.split_once(':').unwrap();
                            if !name.eq_ignore_ascii_case("proxy-authorization") {
                                forwarded.push_str(header);
                                forwarded.push_str("\r\n");
                            }
                        }
                        forwarded.push_str("\r\n");
                        Some(forwarded)
                    };
                    assert!(
                        targets.contains(&target),
                        "only isolated fixture targets may be connected"
                    );
                    requests.lock().unwrap().push(request);
                    let mut origin = TcpStream::connect(target).await.unwrap();
                    if let Some(forwarded) = forwarded {
                        origin.write_all(forwarded.as_bytes()).await.unwrap();
                    } else {
                        stream
                            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                            .await
                            .unwrap();
                    }
                    let _ = tokio::io::copy_bidirectional(&mut stream, &mut origin).await;
                });
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });
        proxy
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
