use super::*;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

async fn request(stream: &mut TcpStream) -> Value {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    let (start, length) = loop {
        let read = stream.read(&mut buffer).await.unwrap();
        assert!(read > 0, "pairing request ended early");
        bytes.extend_from_slice(&buffer[..read]);
        assert!(bytes.len() <= 4096, "pairing request exceeded its bound");
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..index]).unwrap();
            assert!(headers.starts_with("POST /v1/codes/redeem HTTP/1.1\r\n"));
            let length = headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .unwrap()
                .1
                .trim()
                .parse::<usize>()
                .unwrap();
            break (index + 4, length);
        }
    };
    assert!(start + length <= 4096, "pairing body exceeded its bound");
    while bytes.len() < start + length {
        let read = stream.read(&mut buffer).await.unwrap();
        assert!(read > 0, "pairing request body ended early");
        bytes.extend_from_slice(&buffer[..read]);
    }
    serde_json::from_slice(&bytes[start..start + length]).unwrap()
}

#[tokio::test]
async fn retries_the_same_pin_after_lost_reply() {
    let directory = tempfile::tempdir().unwrap();
    let host = Link::controller(directory.path().join("host"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = host.handle().invite().unwrap();
    let expected = host.handle().address().id;
    let ticket = invitation.ticket().to_owned();
    let expires_at_ms = invitation.expires_at_ms();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let service = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await.unwrap();
        let claimed = request(&mut first).await;
        assert_eq!(claimed["code"], "123456");
        // Redemption has consumed the code, but its response never reaches the controller.
        drop(first);
        let (mut retry, _) = listener.accept().await.unwrap();
        let retried = request(&mut retry).await;
        assert_eq!(retried, claimed);
        let body = json!({"ticket": ticket, "expires_at_ms": expires_at_ms}).to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        retry.write_all(response.as_bytes()).await.unwrap();
        retry.shutdown().await.unwrap();
    });
    let controller = Controller::open(
        directory.path().join("controller").to_str().unwrap().into(),
        false,
        vec![],
    )
    .await
    .unwrap();
    assert!(
        controller
            .pair_pin(origin.clone(), "123456".into())
            .await
            .is_err()
    );
    let paired = controller.pair_pin(origin, "123456".into()).await;
    service.await.unwrap();
    controller.close().await.unwrap();
    host.close().await.unwrap();
    let paired: sailry_link::EndpointAddr = serde_json::from_str(&paired.unwrap()).unwrap();
    assert_eq!(paired.id, expected);
}
