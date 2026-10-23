use super::*;
use futures::{StreamExt, stream::FuturesUnordered};

#[tokio::test]
async fn admits_independent_connections() {
    for remote in [false, true] {
        let fixture = fixture::Fixture::new().await;
        let client = fixture.client(remote);
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
        let Output::DatabaseProfile(profile) = execute(
            &client,
            Command::SaveDatabase {
                profile: Profile {
                    id: DatabaseId::new(),
                    revision: 0,
                    sharing: None,
                    name: "Stalled connection".into(),
                    read_only: true,
                    connection: Connection::Network {
                        engine: Engine::Mysql,
                        host: "127.0.0.1".into(),
                        port,
                        database: "fixture".into(),
                        username: "fixture".into(),
                        tls: database::Tls::Disable,
                    },
                },
                expected_revision: 0,
                password: Some(Secret::new("isolated-password".into())),
            },
        )
        .await
        else {
            panic!("database profile expected")
        };
        let mut requests = Vec::new();
        let mut pending = FuturesUnordered::new();
        for _ in 0..10 {
            let request = client.prepare(Command::CheckDatabase {
                profile: profile.id,
                expected_revision: profile.revision,
            });
            requests.push(request.clone());
            pending.push(client.execute(request));
            tokio::select! {
                result = pending.next() => panic!("connection ended before cancellation: {result:?}"),
                ready = tokio::time::timeout(Duration::from_secs(5), accepted.recv()) => {
                    assert_eq!(ready.expect("independent database connection was blocked"), Some(()));
                }
            }
        }
        for request in &requests {
            assert_eq!(
                execute(
                    &client,
                    Command::CancelDatabase {
                        request: request.id
                    }
                )
                .await,
                Output::DatabaseCancelled {
                    request: request.id
                }
            );
        }
        while let Some(result) = pending.next().await {
            assert_eq!(result.unwrap_err().code, ErrorCode::OutcomeUnknown);
        }
        for request in requests {
            assert_eq!(
                client.execute(request).await.unwrap_err().code,
                ErrorCode::OutcomeUnknown
            );
        }
        assert!(accepted.try_recv().is_err());
        connections.abort();
        fixture.close().await;
    }
}
