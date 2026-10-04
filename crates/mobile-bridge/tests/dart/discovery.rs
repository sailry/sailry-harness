use sailry_node_runtime::Node;
use sailry_protocol::conversation::ModelApi;
use serde_json::json;

#[allow(dead_code)]
#[path = "../../../node-runtime/tests/discovery_support/mod.rs"]
pub(super) mod server;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn validates_saved_models() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(server::Server::start(ModelApi::Anthropic, |_| server::Reply::Json(json!({
        "data": [{"id":"known", "max_input_tokens":4096, "max_tokens":128}, {"id":"unknown"}], "has_more":false
    }))));
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let child = super::command("discovery.dart", directory.path(), invitation.ticket())
        .env("SAILRY_PROVIDER_ENDPOINT", &server.endpoint)
        .spawn()
        .unwrap();
    let status = super::wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert!(
        requests
            .iter()
            .all(|request| request.headers["x-api-key"] == "isolated-discovery-key")
    );
}
