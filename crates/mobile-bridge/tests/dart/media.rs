use super::configuration::{provider, support};
use sailry_client::Client;
use sailry_protocol::{Command, Output};

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn approval_and_cancellation() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::tools(vec![(
        support::plugin_tool("media", "generate_video"),
        serde_json::json!({"prompt":"pending","file_name":"pending.mp4"}),
    )]));
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("node"),
        ))
        .unwrap();
    runtime.block_on(async {
        let client = Client::new(node.local());
        let mut provider = provider(&client, &server.endpoint, "ffi-main").await;
        provider.models[0].tools = true;
        client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        let output = client
            .execute(client.prepare(Command::RegisterProject {
                name: "FFI media".into(),
                path: directory.path().to_str().unwrap().into(),
            }))
            .await
            .unwrap();
        assert!(matches!(output, Output::Project(_)));
    });
    let invitation = node.link().invite().unwrap();
    let child = super::command("media.dart", directory.path(), invitation.ticket())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert!(
        !directory
            .path()
            .join("assets/generated/pending.mp4")
            .exists()
    );
}
