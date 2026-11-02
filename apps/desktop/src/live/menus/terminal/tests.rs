use super::*;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{
    Command as NodeCommand,
    process::Access,
    terminal::{Appearance, ColorScheme, Info, Launch, Rgb, Viewport},
};
use std::time::Duration;

async fn snapshot(client: &Client) -> Snapshot {
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(NodeCommand::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    snapshot
}

async fn create(client: &Client, worktree: WorktreeId) -> Info {
    let color = Rgb {
        red: 128,
        green: 128,
        blue: 128,
    };
    let Output::Terminal(info) = client
        .execute(client.prepare(NodeCommand::CreateTerminal(Launch {
            worktree,
            access: Access::Unrestricted,
            viewport: Viewport {
                columns: 80,
                rows: 24,
                pixel_width: 640,
                pixel_height: 384,
            },
            appearance: Appearance {
                foreground: color,
                background: color,
                palette: [color; 16],
                color_scheme: ColorScheme::Dark,
            },
        })))
        .await
        .unwrap()
    else {
        panic!("terminal expected")
    };
    info
}

#[tokio::test]
async fn closes_without_opening_or_claiming() {
    let directory = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let node = {
        let home = directory.path().join("home");
        std::fs::create_dir(&home).unwrap();
        Node::start_empty_with_environment(
            directory.path().join("node"),
            sailry_node_runtime::EnvironmentFixture {
                home,
                shell: "/bin/sh".into(),
                environment: [("PATH".into(), "/usr/bin:/bin".into())].into(),
                timeout: Duration::from_secs(5),
            },
        )
        .await
        .unwrap()
    };
    #[cfg(not(unix))]
    let node = Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let mobile = Link::controller(directory.path().join("mobile"), NetworkScope::default())
        .await
        .unwrap();
    let desktop = Link::controller(directory.path().join("desktop"), NetworkScope::default())
        .await
        .unwrap();
    let mobile_address = mobile
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let desktop_address = desktop
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let mobile_client = Client::new(mobile.handle().remote(mobile_address));
    let transports: [Arc<dyn Transport>; 2] =
        [node.local(), desktop.handle().remote(desktop_address)];
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    mobile_client
        .execute(mobile_client.prepare(NodeCommand::RegisterProject {
            name: "Terminal closure".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let tree = snapshot(&mobile_client).await.worktrees[0].id;
    let untouched = create(&mobile_client, tree).await;
    for transport in transports {
        let client = Client::new(transport);
        let info = create(&mobile_client, tree).await;
        assert_eq!(
            info.owner,
            Some(NodeId(*mobile.handle().address().id.as_bytes()))
        );
        let before = snapshot(&client).await;
        let action = Dispatch {
            node: node.id(),
            target: Target::Terminal(info.id),
            command: Command::Close,
        };
        let command = action.terminal_close(&before).unwrap();
        assert_eq!(
            command,
            NodeCommand::CloseTerminal {
                worktree: tree,
                terminal: info.id
            }
        );
        let mut stale = action.clone();
        stale.node = NodeId([99; 32]);
        assert!(stale.terminal_close(&before).is_none());
        stale = action.clone();
        stale.target = Target::Terminal(TerminalId::new());
        assert!(stale.terminal_close(&before).is_none());
        stale = action.clone();
        stale.command = Command::Open;
        assert!(stale.terminal_close(&before).is_none());

        // The closing controller only reads the inventory; it never opens or claims the PTY.
        let request = client.prepare(command);
        let closed = tokio::time::timeout(Duration::from_secs(5), client.execute(request.clone()))
            .await
            .expect("direct close deadline")
            .unwrap();
        assert!(
            matches!(&closed, Output::Terminal(info) if info.status == Status::Closed && info.owner.is_none())
        );
        assert_eq!(client.execute(request).await.unwrap(), closed);
        let after = snapshot(&client).await;
        assert!(action.terminal_close(&after).is_none());
        assert_eq!(
            after
                .terminals
                .iter()
                .find(|terminal| terminal.id == untouched.id)
                .unwrap()
                .status,
            Status::Running
        );
        assert!(
            matches!(mobile_client.subscribe_terminal(info.id).await, Err(error) if error.code == ErrorCode::NotFound)
        );
    }
    mobile_client
        .execute(mobile_client.prepare(NodeCommand::CloseTerminal {
            worktree: tree,
            terminal: untouched.id,
        }))
        .await
        .unwrap();
    mobile.close().await.unwrap();
    desktop.close().await.unwrap();
    node.shutdown().await.unwrap();
}
