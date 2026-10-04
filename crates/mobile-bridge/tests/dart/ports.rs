#[path = "../../../node-runtime/tests/ports/server.rs"]
#[expect(
    dead_code,
    reason = "Connection counters are exercised by Node port tests; Dart validates forwarding through the bridge"
)]
mod server;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn forwards_and_releases() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("node"),
        ))
        .unwrap();
    let server = runtime.block_on(server::Server::start());
    let invitation = node.link().invite().unwrap();
    let child = super::command("ports.dart", directory.path(), invitation.ticket())
        .env("SAILRY_FORWARD_PORT", server.port.to_string())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    runtime.block_on(server.close());
    assert!(status.success());
}

#[test]
#[cfg(unix)]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn discovers_and_stops_session_services() {
    use super::{configuration::support, tools};
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("project")).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::tools(vec![(
        super::agent_support::plugin_tool("commands", "run_command"),
        serde_json::json!({"background": true, "command": "node -e 'const server=require(\"http\").createServer((q,r)=>r.end(\"service:\"+q.url)); server.listen(0,\"127.0.0.1\",()=>console.log(\"http://localhost:\"+server.address().port+\"/app?q=1\"));'"}),
    )]));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-service",
    ));
    let invitation = node.link().invite().unwrap();
    let child = super::command("services.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSION", sessions[0].to_string())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
}
