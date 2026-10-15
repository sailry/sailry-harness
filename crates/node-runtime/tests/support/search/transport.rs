use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub struct Request {
    pub header: String,
    pub body: Value,
    pub method: String,
    pub path: String,
}

pub async fn read(stream: &mut TcpStream) -> Option<Request> {
    let mut bytes = Vec::new();
    let offset = loop {
        let mut chunk = [0; 8192];
        let count = stream.read(&mut chunk).await.unwrap();
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(offset) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break offset;
        }
    };
    let header = String::from_utf8(bytes[..offset].to_vec()).unwrap();
    let length = header
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .map(|(_, value)| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    while bytes.len() < offset + 4 + length {
        let mut chunk = [0; 8192];
        let count = stream.read(&mut chunk).await.unwrap();
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let body = if length == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&bytes[offset + 4..offset + 4 + length]).unwrap()
    };
    let mut first = header.lines().next().unwrap().split_whitespace();
    let method = first.next().unwrap().to_owned();
    let path = first.next().unwrap().split('?').next().unwrap().to_owned();
    Some(Request {
        header,
        body,
        method,
        path,
    })
}

pub async fn respond(
    stream: &mut TcpStream,
    status: u16,
    retry: Option<&str>,
    body: &str,
    session: Option<&str>,
) {
    let retry = retry
        .map(|value| format!("Retry-After: {value}\r\n"))
        .unwrap_or_default();
    let session = session
        .map(|value| format!("Mcp-Session-Id: {value}\r\n"))
        .unwrap_or_default();
    let response = format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{retry}{session}Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}
