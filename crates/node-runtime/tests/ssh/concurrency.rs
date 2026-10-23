use super::*;
use futures::{StreamExt, stream::FuturesUnordered};

#[tokio::test]
async fn admits_independent_connections() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (entered, mut accepted) = tokio::sync::mpsc::channel(16);
        let connections = tokio::spawn(async move {
            let mut streams = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                streams.push(stream);
                if entered.send(()).await.is_err() {
                    break;
                }
            }
        });
        let profile = save(
            &client,
            profile(port),
            Some(Credential::Password {
                password: Secret::new("isolated-password".into()),
            }),
        )
        .await;
        let mut requests = Vec::new();
        let mut pending = FuturesUnordered::new();
        for _ in 0..10 {
            let request = client.prepare(Command::CheckSsh {
                profile: profile.id,
                expected_revision: profile.revision,
            });
            requests.push(request.clone());
            pending.push(client.execute(request));
            tokio::select! {
                result = pending.next() => panic!("connection ended before cancellation: {result:?}"),
                ready = tokio::time::timeout(Duration::from_secs(5), accepted.recv()) => {
                    assert_eq!(ready.expect("independent SSH connection was blocked"), Some(()));
                }
            }
        }
        for request in &requests {
            assert_eq!(
                execute(
                    &client,
                    Command::CancelSsh {
                        request: request.id
                    }
                )
                .await,
                Output::SshCancelled {
                    request: request.id
                }
            );
        }
        while let Some(result) = pending.next().await {
            assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
        }
        for request in requests {
            assert_eq!(
                client.execute(request).await.unwrap_err().code,
                ErrorCode::Cancelled
            );
        }
        assert!(accepted.try_recv().is_err());
        connections.abort();
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
