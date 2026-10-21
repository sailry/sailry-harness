use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
#[cfg(not(target_os = "macos"))]
use sailry_protocol::Output;
use sailry_protocol::{Command, ErrorCode, computer::Permission};

#[tokio::test]
async fn confines_permission_prompts() {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
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
    #[cfg(not(target_os = "macos"))]
    let mut states = Vec::new();
    for (client, is_local) in [(&local, true), (&remote, false)] {
        let request = client.prepare(Command::ReadComputerPermissions);
        assert!(!request.command.durable());
        let result = client.execute(request).await;
        #[cfg(target_os = "macos")]
        assert_eq!(result.unwrap_err().code, ErrorCode::NotConfigured);
        #[cfg(not(target_os = "macos"))]
        let Output::ComputerPermissions(permissions) = result.unwrap() else {
            panic!("permissions expected")
        };
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(permissions.node, node.id());
            assert_eq!(permissions.local, is_local);
            assert_eq!(permissions.platform, std::env::consts::OS);
            states.push((permissions.screen_capture, permissions.accessibility));
        }
        #[cfg(target_os = "macos")]
        let _ = is_local;
    }
    #[cfg(not(target_os = "macos"))]
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
