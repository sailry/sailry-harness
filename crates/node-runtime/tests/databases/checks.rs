use super::*;
use std::sync::Arc;
use tokio::sync::Notify;

#[tokio::test]
async fn cancels_saved_and_draft_connections() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let accepted = Arc::new(Notify::new());
        let connections = {
            let accepted = accepted.clone();
            tokio::spawn(async move {
                let mut streams = Vec::new();
                while let Ok((stream, _)) = listener.accept().await {
                    streams.push(stream);
                    accepted.notify_one();
                }
            })
        };
        for engine in [Engine::Mysql, Engine::Postgres] {
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
                            engine,
                            host: "127.0.0.1".into(),
                            port,
                            database: "fixture".into(),
                            username: "fixture".into(),
                            tls: database::Tls::Disable,
                        },
                    },
                    expected_revision: 0,
                    password: Some(Secret::new(servers::PASSWORD.into())),
                },
            )
            .await
            else {
                panic!("database profile expected")
            };
            for command in [
                Command::CheckDatabase {
                    profile: profile.id,
                    expected_revision: profile.revision,
                },
                Command::TestDatabase {
                    profile: profile.clone(),
                    expected_revision: profile.revision,
                    password: None,
                },
            ] {
                let request = client.prepare(command);
                let pending = client.execute(request.clone());
                tokio::pin!(pending);
                tokio::select! {
                    result = &mut pending => panic!("connection ended before cancellation: {result:?}"),
                    ready = tokio::time::timeout(Duration::from_secs(2), accepted.notified()) => ready.expect("database connection was not attempted"),
                }
                execute(
                    &client,
                    Command::CancelDatabase {
                        request: request.id,
                    },
                )
                .await;
                let error = tokio::time::timeout(Duration::from_secs(2), pending)
                    .await
                    .unwrap()
                    .unwrap_err();
                assert_eq!(error.code, ErrorCode::OutcomeUnknown);
                assert!(!error.message.contains(servers::PASSWORD));
            }
            execute(
                &client,
                Command::RemoveDatabase {
                    profile: profile.id,
                    expected_revision: profile.revision,
                },
            )
            .await;
        }
        connections.abort();
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

pub(super) async fn cancels_writes(client: &Client, profile: &Profile, table: &str) {
    use sqlx::{ConnectOptions as _, Connection as _, Executor};
    let Connection::Sqlite { path } = &profile.connection else {
        unreachable!()
    };
    let table = format!("{table}_cancelled");
    execute(
        client,
        query(profile, &format!("CREATE TABLE {table}(value INTEGER)")),
    )
    .await;
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .busy_timeout(Duration::ZERO)
        .foreign_keys(false)
        .disable_statement_logging();
    let mut observer = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap();
    let request = client.prepare(query(profile, &format!(
        "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x < 1000000000) INSERT INTO {table} SELECT x FROM n"
    )));
    let pending = client.execute(request.clone());
    tokio::pin!(pending);
    let writing = async {
        loop {
            match observer.execute("BEGIN IMMEDIATE").await {
                Ok(_) => {
                    observer.execute("ROLLBACK").await.unwrap();
                }
                Err(sqlx::Error::Database(error))
                    if error.code().is_some_and(|code| code == "5") =>
                {
                    break;
                }
                Err(error) => panic!("could not observe SQLite writer: {error}"),
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::select! {
        result = &mut pending => panic!("write ended before cancellation: {result:?}"),
        ready = tokio::time::timeout(Duration::from_secs(2), writing) => ready.expect("SQLite write did not start"),
    }
    execute(
        client,
        Command::CancelDatabase {
            request: request.id,
        },
    )
    .await;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(
        client.execute(request).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    observer.close().await.unwrap();
    let mut next = query(profile, &format!("INSERT INTO {table} VALUES (7)"));
    if let Command::QueryDatabase { timeout_ms, .. } = &mut next {
        *timeout_ms = 500;
    }
    execute(client, next).await;
    assert_eq!(
        rows(
            execute(
                client,
                query(profile, &format!("SELECT value FROM {table}"))
            )
            .await
        )
        .rows,
        vec![vec![Value::Integer(7)]]
    );
}
