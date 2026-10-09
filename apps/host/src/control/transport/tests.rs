use super::*;
use std::time::Duration;

#[tokio::test]
async fn resumes_a_partially_consumed_frame() {
    let (mut writer, reader) = tokio::io::duplex(4096);
    let bytes = encode(&Response {
        version: 1,
        node: NodeId([7; 32]),
        body: Reply::Hello,
    })
    .unwrap();
    let middle = bytes.len() / 2;
    writer.write_all(&bytes[..middle]).await.unwrap();
    let mut reader = BufReader::new(reader);
    let mut payload = Vec::new();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            receive_buffered::<Response>(&mut reader, &mut payload)
        )
        .await
        .is_err()
    );
    assert_eq!(payload, bytes[..middle]);
    writer.write_all(&bytes[middle..]).await.unwrap();
    let response: Response = receive_buffered(&mut reader, &mut payload).await.unwrap();
    assert_eq!(response.version, 1);
    assert_eq!(response.node, NodeId([7; 32]));
    assert!(matches!(response.body, Reply::Hello));
    assert!(payload.is_empty());
}

#[test]
fn rejects_wrong_identity_and_unknown_fields() {
    let node = NodeId([7; 32]);
    let other = Response {
        version: 1,
        node: NodeId([8; 32]),
        body: Reply::Hello,
    };
    assert_eq!(
        check(&other, node).unwrap_err().code,
        ErrorCode::WrongTarget
    );
    let version = Response {
        version: 2,
        node,
        body: Reply::Hello,
    };
    assert_eq!(
        check(&version, node).unwrap_err().code,
        ErrorCode::WrongTarget
    );
    assert!(
        serde_json::from_str::<Frame>(r#"{"command":"node_hello","version":1,"fallback":true}"#)
            .is_err()
    );
}
