use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{
    database::{Connection, Engine, Outcome, Profile, Value},
    *,
};
use std::time::Duration;

#[path = "databases/servers.rs"]
mod servers;
#[path = "databases/tunnel.rs"]
mod tunnel;

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}
fn query(profile: &Profile, sql: &str) -> Command {
    Command::QueryDatabase {
        read_only: false,
        database: None,
        profile: profile.id,
        expected_revision: profile.revision,
        sql: sql.into(),
        row_limit: 2,
        timeout_ms: 5000,
    }
}
fn rows(output: Output) -> database::ResultSet {
    let Output::DatabaseOutcome(Outcome::Query(result)) = output else {
        panic!("database rows expected")
    };
    result
}

async fn exercise(connection: Connection, password: Option<Secret>) {
    exercise_transport(connection, password, None).await;
}

async fn exercise_transport(
    connection: Connection,
    password: Option<Secret>,
    tunnel: Option<&tunnel::Server>,
) {
    for remote in [false, true] {
        let forwarded = tunnel.map_or(0, |server| server.forwarded());
        let directory = tempfile::tempdir().unwrap();
        let node_path = directory.path().join("node");
        let node = Node::start(&node_path).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let connection = if let Some(tunnel) = tunnel {
            let ssh = tunnel.save(&client).await;
            let Connection::Network {
                engine,
                host,
                port,
                database,
                username,
                tls,
            } = connection.clone()
            else {
                unreachable!()
            };
            Connection::Ssh {
                engine,
                host,
                port,
                database,
                username,
                tls,
                ssh,
            }
        } else {
            connection.clone()
        };
        let profile = Profile {
            sharing: None,
            id: DatabaseId::new(),
            revision: 0,
            name: "Database fixture".into(),
            connection: connection.clone(),
            read_only: false,
        };
        let draft = client.prepare(Command::TestDatabase {
            profile: profile.clone(),
            expected_revision: 0,
            password: password.clone(),
        });
        assert!(!draft.command.durable());
        assert_eq!(
            client.execute(draft).await.unwrap(),
            Output::DatabaseOutcome(Outcome::Connected)
        );
        assert_eq!(
            execute(&client, Command::ListDatabases).await,
            Output::DatabaseProfiles(vec![])
        );
        let Output::DatabaseProfile(mut profile) = execute(
            &client,
            Command::SaveDatabase {
                profile,
                expected_revision: 0,
                password: password.clone(),
            },
        )
        .await
        else {
            panic!("database profile expected")
        };
        assert_eq!(
            execute(
                &client,
                Command::CheckDatabase {
                    profile: profile.id,
                    expected_revision: profile.revision
                }
            )
            .await,
            Output::DatabaseOutcome(Outcome::Connected)
        );
        let mut draft = profile.clone();
        draft.name = "Unsaved draft".into();
        assert_eq!(
            execute(
                &client,
                Command::TestDatabase {
                    profile: draft,
                    expected_revision: profile.revision,
                    password: None,
                }
            )
            .await,
            Output::DatabaseOutcome(Outcome::Connected)
        );
        assert_eq!(
            execute(&client, Command::ListDatabases).await,
            Output::DatabaseProfiles(vec![profile.clone()])
        );
        if profile.connection.engine() == Engine::Mysql {
            let result = rows(execute(&client, query(&profile,
                "SELECT HOST FROM information_schema.PROCESSLIST WHERE ID = CONNECTION_ID()"
            )).await);
            let Value::Text(peer) = &result.rows[0][0] else {
                panic!("TCP peer expected");
            };
            assert_eq!(
                peer.contains(':'),
                !matches!(profile.connection, Connection::Socket { .. }),
                "unexpected database transport: {peer}"
            );
            let error = client
                .execute(client.prepare(Command::BrowseDatabase {
                    profile: profile.id,
                    expected_revision: profile.revision,
                    database: Some("missing_fixture_database".into()),
                }))
                .await
                .unwrap_err();
            assert!(error.message.contains("1049"), "{error:?}");
            assert!(
                error.message.contains("missing_fixture_database"),
                "{error:?}"
            );
            assert!(!error.message.contains(servers::PASSWORD));
        }
        if profile.connection.engine() == Engine::Postgres {
            let result = rows(
                execute(
                    &client,
                    query(&profile, "SELECT inet_client_addr() IS NULL"),
                )
                .await,
            );
            assert_eq!(
                result.rows[0][0],
                Value::Text(
                    if matches!(profile.connection, Connection::Socket { .. }) {
                        "t"
                    } else {
                        "f"
                    }
                    .into()
                )
            );
        }
        let table = if remote {
            "items_remote"
        } else {
            "items_local"
        };
        execute(
            &client,
            query(
                &profile,
                &format!("CREATE TABLE {table}(id INTEGER, value TEXT)"),
            ),
        )
        .await;
        let database = match &profile.connection {
            Connection::Sqlite { .. } => "main".to_string(),
            Connection::Network { database, .. }
            | Connection::Socket { database, .. }
            | Connection::Ssh { database, .. } => database.clone(),
        };
        let Output::DatabaseOutcome(Outcome::Catalog(database::Catalog::Databases(names))) =
            execute(
                &client,
                Command::BrowseDatabase {
                    profile: profile.id,
                    expected_revision: profile.revision,
                    database: None,
                },
            )
            .await
        else {
            panic!("catalog expected")
        };
        assert!(names.contains(&database));
        let Output::DatabaseOutcome(Outcome::Catalog(database::Catalog::Tables { tables, .. })) =
            execute(
                &client,
                Command::BrowseDatabase {
                    profile: profile.id,
                    expected_revision: profile.revision,
                    database: Some(database.clone()),
                },
            )
            .await
        else {
            panic!("table catalog expected")
        };
        assert!(tables.iter().any(|entry| entry.name == table));
        if !matches!(profile.connection, Connection::Sqlite { .. }) {
            let mut empty = profile.clone();
            empty.id = DatabaseId::new();
            empty.revision = 0;
            if let Connection::Network { database, .. }
            | Connection::Socket { database, .. }
            | Connection::Ssh { database, .. } = &mut empty.connection
            {
                database.clear();
            }
            let Output::DatabaseProfile(empty) = execute(
                &client,
                Command::SaveDatabase {
                    profile: empty,
                    expected_revision: 0,
                    password: password.clone(),
                },
            )
            .await
            else {
                panic!("profile expected")
            };
            assert_eq!(
                execute(
                    &client,
                    Command::CheckDatabase {
                        profile: empty.id,
                        expected_revision: empty.revision
                    }
                )
                .await,
                Output::DatabaseOutcome(Outcome::Connected)
            );
            let Output::DatabaseOutcome(Outcome::Catalog(database::Catalog::Databases(names))) =
                execute(
                    &client,
                    Command::BrowseDatabase {
                        profile: empty.id,
                        expected_revision: empty.revision,
                        database: None,
                    },
                )
                .await
            else {
                panic!("catalog expected")
            };
            assert!(names.contains(&database));
            let mut selected = query(&empty, &format!("SELECT * FROM {table}"));
            if let Command::QueryDatabase {
                database: target, ..
            } = &mut selected
            {
                *target = Some(database);
            }
            assert_eq!(
                rows(execute(&client, selected).await).columns,
                ["id", "value"]
            );
            execute(
                &client,
                Command::RemoveDatabase {
                    profile: empty.id,
                    expected_revision: empty.revision,
                },
            )
            .await;
        }
        let request = client.prepare(query(
            &profile,
            &format!("INSERT INTO {table} VALUES (1, 'value 中文'), (2, NULL), (3, 'last')"),
        ));
        let written = client.execute(request.clone()).await.unwrap();
        assert_eq!(rows(written.clone()).affected_rows, 3);
        assert_eq!(client.execute(request.clone()).await.unwrap(), written);
        let result = rows(
            execute(
                &client,
                query(&profile, &format!("SELECT * FROM {table} ORDER BY id")),
            )
            .await,
        );
        assert_eq!(result.columns, ["id", "value"]);
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][1], Value::Text("value 中文".into()));
        assert_eq!(result.rows[1][1], Value::Null);
        assert!(result.truncated);
        for sql in [
            format!("DELETE FROM {table}"),
            format!("CREATE TABLE {table}_blocked(id INTEGER)"),
        ] {
            let mut read = query(&profile, &sql);
            if let Command::QueryDatabase { read_only, .. } = &mut read {
                *read_only = true;
            }
            assert!(
                client.execute(client.prepare(read)).await.is_err(),
                "read-only operation allowed: {sql}"
            );
        }
        let mut read = query(&profile, &format!("SELECT COUNT(*) FROM {table}"));
        if let Command::QueryDatabase { read_only, .. } = &mut read {
            *read_only = true;
        }
        let count = rows(execute(&client, read).await);
        assert!(
            matches!(&count.rows[0][0], Value::Integer(3))
                || count.rows[0][0] == Value::Text("3".into())
        );
        let mut watcher = client.subscribe().await.unwrap();
        let revision = profile.revision;
        profile.read_only = true;
        let Output::DatabaseProfile(profile) = execute(
            &client,
            Command::SaveDatabase {
                profile,
                expected_revision: revision,
                password: None,
            },
        )
        .await
        else {
            panic!("database profile expected")
        };
        let mut projection = sailry_client::Projection::new(node.id(), 0);
        projection.reconnect(1).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                projection.apply(1, watcher.next().await.unwrap()).unwrap();
                if projection
                    .snapshot()
                    .is_some_and(|snapshot| snapshot.databases == [profile.clone()])
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        let public = serde_json::to_string(projection.snapshot().unwrap()).unwrap();
        assert!(!public.contains("database-fixture-password"));
        assert!(
            client
                .execute(client.prepare(query(&profile, &format!("DELETE FROM {table}"))))
                .await
                .is_err()
        );
        assert!(
            client
                .execute(client.prepare(Command::RemoveDatabase {
                    profile: profile.id,
                    expected_revision: revision
                }))
                .await
                .is_err()
        );
        let count = rows(
            execute(
                &client,
                query(&profile, &format!("SELECT COUNT(*) AS count FROM {table}")),
            )
            .await,
        );
        assert!(
            matches!(&count.rows[0][0], Value::Integer(3))
                || count.rows[0][0] == Value::Text("3".into())
        );
        if matches!(connection, Connection::Sqlite { .. }) {
            let request = client.prepare(query(&profile, "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x < 1000000000) SELECT SUM(x) FROM n"));
            let stopped = request.clone();
            let pending = client.execute(request);
            tokio::pin!(pending);
            tokio::select! {
                result = &mut pending => panic!("query ended before cancellation: {result:?}"),
                _ = tokio::time::sleep(Duration::from_millis(40)) => {},
            }
            execute(
                &client,
                Command::CancelDatabase {
                    request: stopped.id,
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
                client.execute(stopped).await.unwrap_err().code,
                ErrorCode::OutcomeUnknown
            );
        }
        if matches!(profile.connection, Connection::Ssh { .. }) {
            let mut command = query(
                &profile,
                if profile.connection.engine() == Engine::Mysql {
                    "SELECT SLEEP(2)"
                } else {
                    "SELECT pg_sleep(2)"
                },
            );
            if let Command::QueryDatabase { timeout_ms, .. } = &mut command {
                *timeout_ms = 200;
            }
            let error = client.execute(client.prepare(command)).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::OutcomeUnknown);
            assert_eq!(
                rows(execute(&client, query(&profile, "SELECT 1")).await)
                    .rows
                    .len(),
                1
            );
        }
        drop(watcher);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(&node_path).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(client.execute(request).await.unwrap(), written);
        assert_eq!(
            execute(&client, Command::ListDatabases).await,
            Output::DatabaseProfiles(vec![profile.clone()])
        );
        execute(
            &client,
            Command::RemoveDatabase {
                profile: profile.id,
                expected_revision: profile.revision,
            },
        )
        .await;
        assert_eq!(
            execute(&client, Command::ListDatabases).await,
            Output::DatabaseProfiles(vec![])
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        if let Some(tunnel) = tunnel {
            assert!(tunnel.forwarded() > forwarded);
        }
    }
}

#[tokio::test]
async fn sqlite_ownership() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fixture.sqlite3");
    rusqlite::Connection::open(&path).unwrap();
    exercise(
        Connection::Sqlite {
            path: path.to_str().unwrap().into(),
        },
        None,
    )
    .await;
}

#[tokio::test]
async fn stores_inline_passwords() {
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
        for engine in [Engine::Postgres, Engine::Mysql] {
            let Output::DatabaseProfile(profile) = execute(
                &client,
                Command::SaveDatabase {
                    profile: Profile {
                        sharing: None,
                        id: DatabaseId::new(),
                        revision: 0,
                        name: "Inline fixture".into(),
                        connection: Connection::Network {
                            engine,
                            host: "127.0.0.1".into(),
                            port: 9,
                            database: "fixture".into(),
                            username: "fixture".into(),
                            tls: database::Tls::Disable,
                        },
                        read_only: true,
                    },
                    expected_revision: 0,
                    password: Some(Secret::new("inline-database-password".into())),
                },
            )
            .await
            else {
                panic!("database profile expected")
            };
            let db =
                rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
            let password: String = db
                .query_row(
                    "SELECT password FROM database_profiles WHERE id=?1",
                    [profile.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(password, "inline-database-password");
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires initdb, postgres, and mysqld binaries; creates isolated local servers"]
async fn network_engines() {
    for engine in [Engine::Postgres, Engine::Mysql] {
        let server = servers::Server::start(engine).await;
        let mut connection = server.connection.clone();
        if let Connection::Network {
            engine: Engine::Mysql,
            host,
            ..
        } = &mut connection
        {
            *host = "localhost".into();
        }
        exercise(connection, Some(Secret::new(servers::PASSWORD.into()))).await;
    }
}

#[tokio::test]
#[ignore = "requires initdb, postgres, and mysqld binaries; creates isolated socket servers"]
async fn socket_engines() {
    for engine in [Engine::Postgres, Engine::Mysql] {
        let server = servers::Server::start(engine).await;
        exercise(server.socket(), Some(Secret::new(servers::PASSWORD.into()))).await;
    }
}

#[tokio::test]
#[ignore = "requires initdb, postgres, and mysqld binaries; creates isolated tunneled servers"]
async fn ssh_engines() {
    for engine in [Engine::Postgres, Engine::Mysql] {
        let server = servers::Server::start(engine).await;
        let tunnel = tunnel::Server::start(&server.connection).await;
        exercise_transport(
            server.connection.clone(),
            Some(Secret::new(servers::PASSWORD.into())),
            Some(&tunnel),
        )
        .await;
    }
}
