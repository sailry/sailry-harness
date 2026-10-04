use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) struct Pages {
    pub url: String,
    pub upload: Arc<Mutex<Vec<u8>>>,
    pub waiting: Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Pages {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Pages {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let upload = Arc::new(Mutex::new(Vec::new()));
        let received = upload.clone();
        let waiting = Arc::new(tokio::sync::Notify::new());
        let started = waiting.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let received = received.clone();
                let started = started.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let (end, length) = loop {
                        let mut chunk = [0; 8192];
                        let count = stream.read(&mut chunk).await.unwrap_or(0);
                        if count == 0 {
                            return;
                        }
                        request.extend_from_slice(&chunk[..count]);
                        if request.len() > 1024 * 1024 {
                            return;
                        }
                        if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&request[..end]);
                            let length = headers
                                .lines()
                                .find_map(|line| {
                                    line.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .and_then(|value| value.trim().parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            break (end + 4, length);
                        }
                    };
                    if length > 1024 * 1024 {
                        return;
                    }
                    while request.len() < end + length {
                        let mut chunk = [0; 8192];
                        let count = stream.read(&mut chunk).await.unwrap_or(0);
                        if count == 0 {
                            return;
                        }
                        request.extend_from_slice(&chunk[..count]);
                    }
                    let path = String::from_utf8_lossy(&request[..end])
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .to_owned();
                    let (body, headers) = match path.as_str() {
                        "/waiting" => {
                            started.notify_one();
                            ("Waiting".into(), "Content-Type: text/plain\r\n")
                        }
                        "/download" => (
                            "download fixture".to_string(),
                            "Content-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=fixture.txt\r\n",
                        ),
                        "/upload" => {
                            *received.lock().unwrap() = request[end..end + length].to_vec();
                            ("Uploaded".into(), "Content-Type: text/plain\r\n")
                        }
                        "/frame" => (
                            "<input id='inside'><p>Frame ready</p>".into(),
                            "Content-Type: text/html; charset=utf-8\r\n",
                        ),
                        _ => (
                            format!(
                                r#"<!doctype html><title>Browser fixture</title>
<form onsubmit="event.preventDefault();document.querySelector('#result').textContent=document.querySelector('#name').value+':'+document.querySelector('#choice').value">
<input id="name"><select id="choice"><option value="a">A</option><option value="b">B</option></select><button id="submit">Submit</button></form><p id="result">Ready</p>
<iframe id="frame" src="http://localhost:{port}/frame"></iframe>
<input id="upload" type="file" onchange="this.files[0].text().then(text=>fetch('/upload',{{method:'POST',body:text}})).then(()=>document.querySelector('#status').textContent='Uploaded')"><p id="status"></p>
<a id="download" href="/download">Download</a>"#
                            ),
                            "Content-Type: text/html; charset=utf-8\r\n",
                        ),
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self {
            url: format!("http://127.0.0.1:{port}"),
            upload,
            waiting,
            task,
        }
    }
}
