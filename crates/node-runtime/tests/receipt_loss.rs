//! Real transport fault injection; these tests do not simulate business execution.
use iroh::{Endpoint, RelayMode, endpoint::presets};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, ErrorCode, Fault, NodeId};

async fn response_fault(reply: Option<serde_json::Value>, expected: ErrorCode) {
    let directory = tempfile::tempdir().unwrap();
    let controller = Node::start(directory.path().join("controller"))
        .await
        .unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .clear_ip_transports()
        .bind_addr((std::net::Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .alpns(vec![b"sailry/node/1".to_vec()])
        .bind()
        .await
        .unwrap();
    controller
        .link()
        .set_trust(NodeId(*endpoint.id().as_bytes()), true)
        .await
        .unwrap();
    let client = Client::new(controller.link().remote(endpoint.addr()));
    let request = client.prepare(Command::RegisterProject {
        name: "Uncertain".into(),
        path: directory.path().to_str().unwrap().into(),
    });
    let original = request.clone();
    let server = async {
        let connection = endpoint.accept().await.unwrap().await.unwrap();
        let (mut send, mut recv) = connection.accept_bi().await.unwrap();
        let mut prefix = [0; 4];
        recv.read_exact(&mut prefix).await.unwrap();
        let mut bytes = vec![0; u32::from_be_bytes(prefix) as usize];
        recv.read_exact(&mut bytes).await.unwrap();
        let call: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(call["data"], serde_json::to_value(original).unwrap());
        if let Some(reply) = reply {
            let bytes = serde_json::to_vec(&reply).unwrap();
            send.write_all(&(bytes.len() as u32).to_be_bytes())
                .await
                .unwrap();
            send.write_all(&bytes).await.unwrap();
            send.finish().unwrap();
            let _ = send.stopped().await;
        } else {
            send.reset(0u32.into()).unwrap();
            let _ = connection.closed().await;
        }
    };
    let caller = async {
        let error = match client.dispatch(request).await {
            Ok(_) => panic!("fault expected"),
            Err(error) => error,
        };
        assert_eq!(error.code, expected);
        controller
            .link()
            .disconnect(NodeId(*endpoint.id().as_bytes()))
            .await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(server, caller);
    })
    .await
    .unwrap();
    endpoint.close().await;
    controller.shutdown().await.unwrap();
}

#[tokio::test]
async fn reset_before_receipt_is_uncertain() {
    response_fault(None, ErrorCode::OutcomeUnknown).await;
}

#[tokio::test]
async fn malformed_receipt_is_uncertain() {
    response_fault(
        Some(serde_json::json!({"kind": "invalid"})),
        ErrorCode::OutcomeUnknown,
    )
    .await;
}

#[tokio::test]
async fn unrelated_receipt_is_uncertain() {
    response_fault(
        Some(serde_json::json!({
            "kind": "receipt",
            "data": { "id": sailry_protocol::RequestId::new(), "durable": true }
        })),
        ErrorCode::OutcomeUnknown,
    )
    .await;
}

#[tokio::test]
async fn explicit_rejection_is_preserved() {
    response_fault(
        Some(serde_json::json!({
            "kind": "rejected",
            "data": Fault::new(ErrorCode::Busy, "admission full")
        })),
        ErrorCode::Busy,
    )
    .await;
}
