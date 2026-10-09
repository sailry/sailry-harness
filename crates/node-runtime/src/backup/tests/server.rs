//! Loopback WebDAV/S3 fixtures exercise the actual storage adapters.
use sailry_protocol::backup::{Destination, Kind};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub(crate) struct Server {
    pub endpoint: String,
    kind: Kind,
    state: Arc<State>,
    task: tokio::task::JoinHandle<()>,
}

struct State {
    kind: Kind,
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    puts: AtomicUsize,
    maximum: AtomicUsize,
}

impl Server {
    pub(crate) async fn start(kind: Kind) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(State {
            kind,
            files: Mutex::default(),
            puts: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
        });
        let captured = state.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    break;
                };
                let state = captured.clone();
                tokio::spawn(async move { handle(socket, state).await });
            }
        });
        Self {
            endpoint,
            kind,
            state,
            task,
        }
    }

    pub(crate) fn destination(&self) -> Destination {
        Destination {
            kind: self.kind,
            endpoint: self.endpoint.clone(),
            root: "profiles".into(),
            bucket: "bucket".into(),
            region: "us-east-1".into(),
        }
    }
    pub(crate) fn writes(&self) -> usize {
        self.state.puts.load(Ordering::SeqCst)
    }
    pub(crate) fn maximum_put(&self) -> usize {
        self.state.maximum.load(Ordering::SeqCst)
    }
    pub(crate) fn parts(&self, id: &str) -> usize {
        self.state
            .files
            .lock()
            .unwrap()
            .keys()
            .filter(|path| path.contains(&format!("/{id}/")) && path.ends_with(".bin"))
            .count()
    }
    pub(crate) fn corrupt(&self, id: &str) {
        let mut files = self.state.files.lock().unwrap();
        let key = files
            .keys()
            .find(|path| path.contains(&format!("/{id}/")) && path.ends_with(".bin"))
            .cloned()
            .unwrap();
        files.get_mut(&key).unwrap()[0] ^= 1;
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn handle(mut socket: TcpStream, state: Arc<State>) {
    let mut bytes = Vec::new();
    let header = loop {
        let mut buffer = [0; 8192];
        let Ok(length) = socket.read(&mut buffer).await else {
            return;
        };
        if length == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..length]);
        if bytes.len() > 5 * 1024 * 1024 {
            return;
        }
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let text = String::from_utf8(bytes[..header].to_vec()).unwrap();
    let mut request = text.lines().next().unwrap().split_whitespace();
    let method = request.next().unwrap();
    let path = request.next().unwrap();
    let fields: BTreeMap<String, String> = text
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.to_ascii_lowercase(), value.trim().into()))
        .collect();
    let length = fields
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    if length > 5 * 1024 * 1024 {
        return;
    }
    while bytes.len() - header < length {
        let mut buffer = [0; 8192];
        let Ok(count) = socket.read(&mut buffer).await else {
            return;
        };
        if count == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let url = reqwest::Url::parse(&format!("http://fixture{path}")).unwrap();
    let path = url.path();
    let authenticated = match state.kind {
        Kind::Webdav => fields
            .get("authorization")
            .is_some_and(|value| value == "Basic Zml4dHVyZS11c2VyOmZpeHR1cmUtcGFzc3dvcmQ="),
        Kind::S3 => fields
            .get("authorization")
            .is_some_and(|value| value.starts_with("AWS4-HMAC-SHA256 Credential=fixture-user/")),
    };
    if !authenticated {
        reply(&mut socket, 403, &[], &[]).await;
        return;
    }
    match method {
        "MKCOL" => reply(&mut socket, 201, &[], &[]).await,
        "PUT" => {
            state.puts.fetch_add(1, Ordering::SeqCst);
            state.maximum.fetch_max(length, Ordering::SeqCst);
            state
                .files
                .lock()
                .unwrap()
                .insert(path.into(), bytes[header..header + length].to_vec());
            reply(&mut socket, 201, &[("ETag", "\"fixture\"".into())], &[]).await;
        }
        "DELETE" => {
            state.files.lock().unwrap().remove(path);
            reply(&mut socket, 204, &[], &[]).await;
        }
        "HEAD" => {
            let length = state.files.lock().unwrap().get(path).map(Vec::len);
            if let Some(length) = length {
                reply(
                    &mut socket,
                    200,
                    &[
                        ("Content-Length", length.to_string()),
                        ("ETag", "\"fixture\"".into()),
                    ],
                    &[],
                )
                .await;
            } else {
                reply(&mut socket, 404, &[], &[]).await;
            }
        }
        "PROPFIND" => {
            let files = state.files.lock().unwrap().clone();
            let mut responses = vec![property(path, None)];
            if fields.get("depth").is_some_and(|value| value == "0") {
                let Some(body) = files.get(path) else {
                    reply(&mut socket, 404, &[], &[]).await;
                    return;
                };
                responses = vec![property(path, Some(body.len()))];
            } else {
                let prefix = path.trim_end_matches('/').to_owned() + "/";
                for (key, body) in &files {
                    if key.starts_with(&prefix) && !key[prefix.len()..].contains('/') {
                        responses.push(property(key, Some(body.len())));
                    }
                }
            }
            let body = format!(
                "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\">{}</d:multistatus>",
                responses.join("")
            );
            reply(
                &mut socket,
                207,
                &[("Content-Type", "application/xml".into())],
                body.as_bytes(),
            )
            .await;
        }
        "GET" if url.query_pairs().any(|(key, _)| key == "list-type") => {
            let prefix = url
                .query_pairs()
                .find(|(key, _)| key == "prefix")
                .map(|(_, value)| value.into_owned())
                .unwrap_or_default();
            let files = state.files.lock().unwrap().clone();
            let mut contents = String::new();
            for (key, body) in &files {
                let Some(key) = key.strip_prefix("/bucket/") else {
                    continue;
                };
                if key.starts_with(&prefix) && !key[prefix.len()..].contains('/') {
                    contents.push_str(&format!("<Contents><Key>{key}</Key><LastModified>2026-10-10T00:00:00Z</LastModified><ETag>\"fixture\"</ETag><Size>{}</Size></Contents>", body.len()));
                }
            }
            let body = format!(
                "<ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Name>bucket</Name><Prefix>{prefix}</Prefix><IsTruncated>false</IsTruncated>{contents}</ListBucketResult>"
            );
            reply(
                &mut socket,
                200,
                &[("Content-Type", "application/xml".into())],
                body.as_bytes(),
            )
            .await;
        }
        "GET" => {
            let body = state.files.lock().unwrap().get(path).cloned();
            if let Some(body) = body {
                let range = fields
                    .get("range")
                    .and_then(|value| value.strip_prefix("bytes="))
                    .and_then(|value| value.split_once('-'));
                if let Some((start, end)) = range {
                    let start = start.parse::<usize>().unwrap();
                    let end = if end.is_empty() {
                        body.len() - 1
                    } else {
                        end.parse::<usize>().unwrap().min(body.len() - 1)
                    };
                    reply(
                        &mut socket,
                        206,
                        &[(
                            "Content-Range",
                            format!("bytes {start}-{end}/{}", body.len()),
                        )],
                        &body[start..=end],
                    )
                    .await;
                } else {
                    reply(&mut socket, 200, &[], &body).await;
                }
            } else {
                reply(&mut socket, 404, &[], &[]).await;
            }
        }
        _ => reply(&mut socket, 405, &[], &[]).await,
    }
}

fn property(path: &str, size: Option<usize>) -> String {
    let kind = if size.is_none() {
        "<d:collection/>"
    } else {
        ""
    };
    format!(
        "<d:response><d:href>{path}</d:href><d:propstat><d:prop><d:resourcetype>{kind}</d:resourcetype><d:getcontentlength>{}</d:getcontentlength><d:getetag>\"fixture\"</d:getetag><d:getlastmodified>Sat, 10 Oct 2026 00:00:00 GMT</d:getlastmodified></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
        size.unwrap_or(0)
    )
}

async fn reply(socket: &mut TcpStream, status: u16, headers: &[(&str, String)], body: &[u8]) {
    let length = headers
        .iter()
        .find(|(key, _)| *key == "Content-Length")
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| body.len().to_string());
    let mut head =
        format!("HTTP/1.1 {status} Fixture\r\nContent-Length: {length}\r\nConnection: close\r\n");
    for (key, value) in headers {
        if *key != "Content-Length" {
            head.push_str(&format!("{key}: {value}\r\n"));
        }
    }
    head.push_str("\r\n");
    let _ = socket.write_all(head.as_bytes()).await;
    let _ = socket.write_all(body).await;
    let _ = socket.shutdown().await;
}
