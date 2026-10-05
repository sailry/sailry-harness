use crate::file_fixture as files;
use sailry_client::ports::State;
use sailry_protocol::{Command, ErrorCode, Output};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "ports/server.rs"]
mod server;

#[tokio::test]
async fn releases_half_closed_routes() {
    for remote in [false, true] {
        let fixture = files::Fixture::start().await;
        let server = server::Server::start().await;
        let clients = fixture.clients();
        let client = &clients[usize::from(remote)];
        let other = &clients[usize::from(!remote)];
        let Output::PortStream { stream } = client
            .execute(client.prepare(Command::OpenPort { port: server.port }))
            .await
            .unwrap()
        else {
            panic!("port stream expected")
        };
        assert_eq!(
            other.open(stream).await.err().unwrap().code,
            ErrorCode::PermissionDenied
        );
        let mut direct = client.open(stream).await.unwrap();
        client
            .execute(client.prepare(Command::CancelPort { stream }))
            .await
            .unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(3), direct.read_u8())
            .await
            .unwrap();
        drop(direct);

        let forwarder = client.forward_port(server.port, 0).await.unwrap();
        let local_port = forwarder.local_port;
        let payload: Vec<_> = (0..2 * 1024 * 1024).map(|n| (n % 251) as u8).collect();
        let mut tcp = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, local_port))
            .await
            .unwrap();
        tcp.write_all(&payload).await.unwrap();
        tcp.shutdown().await.unwrap();
        let mut received = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), tcp.read_to_end(&mut received))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, payload);
        server.wait_idle().await;

        let mut held = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, local_port))
            .await
            .unwrap();
        held.write_all(b"hold").await.unwrap();
        server.wait_active().await;
        forwarder.close().await;
        let _ = tokio::time::timeout(Duration::from_secs(3), held.read_u8())
            .await
            .unwrap();
        server.wait_idle().await;
        let rebound = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, local_port))
            .await
            .unwrap();
        drop(rebound);

        let forwarder = client.forward_port(server.port, 0).await.unwrap();
        let mut state = forwarder.state.clone();
        let mut node = Some(fixture.node);
        let mut controller = Some(fixture.controller);
        if remote {
            controller.take().unwrap().close().await.unwrap();
        } else {
            node.take().unwrap().shutdown().await.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            while *state.borrow() == State::Listening {
                state.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(matches!(*state.borrow(), State::Failed(_)));
        forwarder.close().await;
        if let Some(node) = node {
            node.shutdown().await.unwrap();
        }
        if let Some(controller) = controller {
            controller.close().await.unwrap();
        }
        server.close().await;
    }
}

#[tokio::test]
async fn belongs_to_host_without_projects() {
    for remote in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let node = sailry_node_runtime::Node::start(temp.path().join("node"))
            .await
            .unwrap();
        let controller = sailry_link::Link::controller(
            temp.path().join("controller"),
            sailry_link::NetworkScope::default(),
        )
        .await
        .unwrap();
        controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = sailry_client::Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let server = server::Server::start().await;
        let forwarder = client.forward_port(server.port, 0).await.unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let Output::Project(project) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Unrelated project".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        client
            .execute(client.prepare(Command::RemoveProject { expected: project }))
            .await
            .unwrap();
        let mut socket =
            tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, forwarder.local_port))
                .await
                .unwrap();
        socket.write_all(b"host-owned route").await.unwrap();
        socket.shutdown().await.unwrap();
        let mut received = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), socket.read_to_end(&mut received))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, b"host-owned route");
        assert_eq!(*forwarder.state.borrow(), State::Listening);
        forwarder.close().await;
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        server.close().await;
    }
}
