use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, ErrorCode, Output, computer::Permission};

#[tokio::test]
async fn confines_permission_prompts() {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let local = Client::new(node.local());
    let remote = Client::new(controller.handle().remote(address));
    let mut states = Vec::new();
    for (client, is_local) in [(&local, true), (&remote, false)] {
        let request = client.prepare(Command::ReadComputerPermissions);
        assert!(!request.command.durable());
        let Output::ComputerPermissions(permissions) = client.execute(request).await.unwrap()
        else {
            panic!("permissions expected")
        };
        assert_eq!(permissions.node, node.id());
        assert_eq!(permissions.local, is_local);
        assert_eq!(permissions.platform, std::env::consts::OS);
        states.push((permissions.screen_capture, permissions.accessibility));
    }
    assert_eq!(states[0], states[1]);
    for permission in [Permission::ScreenCapture, Permission::Accessibility] {
        let request = remote.prepare(Command::RequestComputerPermission { permission });
        assert!(
            !request.command.durable(),
            "system prompts must not be replayed"
        );
        let error = remote.execute(request).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
    }
    drop(local);
    drop(remote);
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
