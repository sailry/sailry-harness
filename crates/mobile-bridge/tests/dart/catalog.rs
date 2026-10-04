use super::discovery::server::{Reply, Server};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output, conversation::ModelApi};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn refreshes_and_resumes() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let count = AtomicUsize::new(0);
    let server = runtime.block_on(Server::start(ModelApi::Anthropic, move |_| {
        if count.fetch_add(1, Ordering::SeqCst) == 2 {
            Reply::Json(json!({"invalid":true}))
        } else {
            Reply::Json(json!({"anthropic":{"models":{
                "known":{"id":"known", "limit":{"context":8192,"output":1024},
                    "modalities":{"input":["text","image"]}, "tool_call":true, "reasoning":true,
                    "reasoning_options":[{"type":"budget_tokens","min":1024}]},
                "unknown":{"id":"unknown"}
            }}}))
        }
    }));
    let profile = directory.path().join("node");
    let node = runtime
        .block_on(Node::start_with_catalog(&profile, &server.endpoint))
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let child = super::command("catalog.dart", directory.path(), invitation.ticket())
        .spawn()
        .unwrap();
    let result = super::wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(result.success());
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| !request.headers.contains_key("authorization"))
    );
    runtime.block_on(async {
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(node.local());
        let Output::CatalogStatus(status) = client
            .execute(client.prepare(Command::ReadCatalogStatus))
            .await
            .unwrap()
        else {
            panic!("catalog status expected")
        };
        assert_eq!((status.revision, status.models), (2, 2));
        node.shutdown().await.unwrap();
    });
}
