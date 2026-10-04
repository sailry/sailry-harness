use super::*;
use rmcp::{
    model::{ClientJsonRpcMessage, ClientRequest, PingRequest, RequestId},
    transport::streamable_http_client::{
        StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
    },
};
use std::collections::HashMap;

const LIMIT: usize = 1024;

async fn response(
    head: String,
    body: Vec<u8>,
    hold: bool,
) -> Result<StreamableHttpPostResponse, StreamableHttpError<reqwest::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri = format!("http://{}/mcp", listener.local_addr().unwrap());
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 8192];
        loop {
            let count = stream.read(&mut buffer).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&buffer[..count]);
            if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                let length: usize = std::str::from_utf8(&request[..end])
                    .unwrap()
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(&body).await.unwrap();
        if hold {
            // No EOF or terminal chunk: the client must reject and close first.
            assert_eq!(stream.read(&mut buffer).await.unwrap_or(0), 0);
        }
    });
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        reqwest::Client::new().post_message_with_max_sse_event_size(
            uri.into(),
            ClientJsonRpcMessage::request(
                ClientRequest::PingRequest(PingRequest::default()),
                RequestId::Number(1),
            ),
            None,
            None,
            HashMap::new(),
            LIMIT,
        ),
    )
    .await
    .expect("bounded response must finish before the server closes");
    tokio::time::timeout(Duration::from_secs(3), peer)
        .await
        .expect("response connection must close")
        .unwrap();
    result
}

fn head(status: u16, content_type: &str, framing: &str) -> String {
    format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Type: {content_type}\r\n{framing}\r\nConnection: close\r\n\r\n"
    )
}

#[tokio::test]
async fn rejects_oversized_bodies() {
    for (status, content_type) in [
        (200, "application/json"),
        (400, "application/json"),
        (500, "text/plain"),
    ] {
        for chunked in [false, true] {
            let framing = if chunked {
                "Transfer-Encoding: chunked".into()
            } else {
                format!("Content-Length: {}", LIMIT + 1)
            };
            let body = if chunked {
                format!("{:x}\r\n{}\r\n", LIMIT + 1, "x".repeat(LIMIT + 1)).into_bytes()
            } else {
                Vec::new()
            };
            let error = response(head(status, content_type, &framing), body, true)
                .await
                .unwrap_err();
            assert!(matches!(
                error,
                StreamableHttpError::UnexpectedServerResponse(_)
            ));
            assert!(error.to_string().contains("maximum message size"));
        }
    }
}

#[tokio::test]
async fn accepts_limit_sized_messages() {
    for (status, value) in [
        (200, json!({"jsonrpc":"2.0", "id":1, "result":{}})),
        (
            400,
            json!({"jsonrpc":"2.0", "id":1, "error":{"code":-32600, "message":"完整结果 🙂"}}),
        ),
    ] {
        let mut body = serde_json::to_vec(&value).unwrap();
        body.resize(LIMIT, b' ');
        for chunked in [false, true] {
            let (framing, body) = if chunked {
                let mut framed = Vec::new();
                for chunk in body.chunks(7) {
                    framed.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
                    framed.extend_from_slice(chunk);
                    framed.extend_from_slice(b"\r\n");
                }
                framed.extend_from_slice(b"0\r\n\r\n");
                ("Transfer-Encoding: chunked".into(), framed)
            } else {
                (format!("Content-Length: {LIMIT}"), body.clone())
            };
            let result = response(head(status, "application/json", &framing), body, false)
                .await
                .unwrap();
            let StreamableHttpPostResponse::Json(message, _) = result else {
                panic!("complete JSON-RPC response expected");
            };
            assert_eq!(serde_json::to_value(message).unwrap(), value);
        }
    }
}

#[tokio::test]
async fn reports_truncated_bodies() {
    for status in [200, 500] {
        let error = response(
            head(status, "application/json", "Content-Length: 100"),
            b"{\"jsonrpc\":".to_vec(),
            false,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, StreamableHttpError::Client(_)));
    }
}

#[tokio::test]
async fn preserves_per_event_stream_limits() {
    use futures::StreamExt;
    let event = format!("data: {}\n\n", "x".repeat(LIMIT - 20));
    let body = event.repeat(2).into_bytes();
    let framing = format!("Content-Length: {}", body.len());
    let StreamableHttpPostResponse::Sse(mut stream, _) =
        response(head(200, "text/event-stream", &framing), body, false)
            .await
            .unwrap()
    else {
        panic!("SSE stream expected");
    };
    for _ in 0..2 {
        assert_eq!(
            stream.next().await.unwrap().unwrap().data.unwrap().len(),
            LIMIT - 20
        );
    }
    assert!(stream.next().await.is_none());
}

pub(super) async fn oversized(stream: &mut TcpStream, chunked: bool) -> std::io::Result<()> {
    let length = 8 * 1024 * 1024 + 1;
    let framing = if chunked {
        "Transfer-Encoding: chunked".into()
    } else {
        format!("Content-Length: {length}")
    };
    stream
        .write_all(head(200, "application/json", &framing).as_bytes())
        .await?;
    if chunked {
        let block = vec![b' '; 8192];
        for offset in (0..length).step_by(block.len()) {
            let count = (length - offset).min(block.len());
            stream
                .write_all(format!("{count:x}\r\n").as_bytes())
                .await?;
            stream.write_all(&block[..count]).await?;
            stream.write_all(b"\r\n").await?;
        }
        stream.write_all(b"0\r\n\r\n").await?;
    }
    Ok(())
}

#[tokio::test]
async fn isolates_failed_discovery() {
    for remote in [false, true] {
        for method in ["tools/list", "tools/call"] {
            for chunked in [false, true] {
                let http = Http::start(Reply::Oversized { method, chunked }).await;
                let calls = if method == "tools/list" {
                    vec![("load_skill".into(), json!({"skill":"example:analysis"}))]
                } else {
                    vec![(alias("remote", "write"), json!({"value":"once"}))]
                };
                let model = Server::tools(calls).await;
                let fixture = process::Fixture::new(remote, &model).await;
                package(
                    &fixture.root,
                    json!({"remote":{"type":"streamable-http", "url":http.endpoint}}),
                    "bounded",
                );
                install(&fixture, 0).await;

                let (request, turn) = submit(&fixture, false).await;
                if method == "tools/call" {
                    let (_, approval) =
                        approvals::pending(&fixture.client, fixture.session.id).await;
                    process::decide(&fixture.client, &approval, Decision::Approve).await;
                }
                let page = finished(&fixture.client, fixture.session.id, turn.id).await;
                assert_eq!(page.runs[0].status, Status::Completed);
                let output = results(&page);
                if method == "tools/list" {
                    assert!(
                        output[0]["content"]
                            .as_str()
                            .unwrap()
                            .contains("Complete skill beside MCP")
                    );
                } else {
                    assert_eq!(output[0]["error"]["code"], "outcome_unknown");
                }
                fixture.client.execute(request).await.unwrap();
                assert_eq!(http.count("initialize"), 1);
                assert_eq!(
                    http.count("tools/list"),
                    if method == "tools/list" { 1 } else { 2 }
                );
                assert_eq!(
                    http.count("tools/call"),
                    usize::from(method == "tools/call")
                );
                assert_eq!(
                    http.effects.load(Ordering::SeqCst),
                    usize::from(method == "tools/call")
                );
                assert_eq!(http.deletes(), 1);
                fixture.controller.close().await.unwrap();
                fixture.node.shutdown().await.unwrap();
            }
        }
    }
}
