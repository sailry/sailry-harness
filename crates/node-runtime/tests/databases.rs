use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{
    database::{Connection, Engine, Outcome, Profile, Value},
    *,
};
use std::time::Duration;

#[path = "databases/checks.rs"]
mod checks;
#[path = "databases/servers.rs"]
mod servers;
#[path = "databases/tls.rs"]
mod tls;
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
    exercise_transport(connection, password, None, true).await;
}

async fn exercise_transport(
    connection: Connection,
    password: Option<Secret>,
    tunnel: Option<&tunnel::Server>,
    full: bool,
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
        if !matches!(profile.connection, Connection::Sqlite { .. }) {
            let mut draft = profile.clone();
            match &mut draft.connection {
                Connection::Network { tls, .. } | Connection::Ssh { tls, .. } => {
                    *tls = database::Tls::Require
                }
                _ => {}
            }
            if !matches!(draft.connection, Connection::Socket { .. }) {
                let error = client
                    .execute(client.prepare(Command::TestDatabase {
                        profile: draft,
                        expected_revision: profile.revision,
                        password: None,
                    }))
                    .await
                    .unwrap_err();
                assert_eq!(error.code, ErrorCode::Unavailable);
                assert!(
                    error.message.to_ascii_lowercase().contains("tls"),
                    "{error:?}"
                );
                assert!(!error.message.contains(servers::PASSWORD));
            }
        }
        if !full {
            let transport = if tunnel.is_some() { "ssh" } else { "socket" };
            let table = format!(
                "transport_{transport}_{}",
                if remote { "remote" } else { "local" }
            );
            execute(
                &client,
                query(&profile, &format!("CREATE TABLE {table}(value INTEGER)")),
            )
            .await;
            execute(
                &client,
                query(&profile, &format!("INSERT INTO {table} VALUES (42)")),
            )
            .await;
            let value = rows(
                execute(
                    &client,
                    query(&profile, &format!("SELECT value FROM {table}")),
                )
                .await,
            );
            assert!(
                matches!(value.rows[0][0], Value::Integer(42))
                    || value.rows[0][0] == Value::Text("42".into())
            );
            let database = profile.connection.server().unwrap().database.to_owned();
            let catalog = execute(
                &client,
                Command::BrowseDatabase {
                    profile: profile.id,
                    expected_revision: profile.revision,
                    database: Some(database),
                },
            )
            .await;
            assert!(
                matches!(catalog, Output::DatabaseOutcome(Outcome::Catalog(database::Catalog::Tables { tables, .. })) if tables.iter().any(|entry| entry.name == table))
            );
            if tunnel.is_some() {
                timeout(&client, &profile).await;
            }
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
            if let Some(tunnel) = tunnel {
                assert!(tunnel.forwarded() > forwarded);
            }
            continue;
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
        if profile.connection.engine() == Engine::Mysql {
            execute(
                &client,
                query(
                    &profile,
                    &format!("CREATE TABLE {table}_flags(enabled BOOLEAN, tiny TINYINT(1))"),
                ),
            )
            .await;
            execute(
                &client,
                query(
                    &profile,
                    &format!("INSERT INTO {table}_flags VALUES (1, 0)"),
                ),
            )
            .await;
            assert_eq!(
                rows(
                    execute(
                        &client,
                        query(
                            &profile,
                            &format!("SELECT enabled, tiny FROM {table}_flags")
                        )
                    )
                    .await
                )
                .rows,
                vec![vec![Value::Integer(1), Value::Integer(0)]]
            );
        }
        let (sql, expected) = match profile.connection.engine() {
            Engine::Sqlite => (
                "SELECT 9223372036854775807, 1.25, CAST(X'ff' AS TEXT), X'00ff', NULL",
                vec![
                    Value::Integer(i64::MAX),
                    Value::Real("1.25".into()),
                    Value::Text("�".into()),
                    Value::Blob(vec![0, 255]),
                    Value::Null,
                ],
            ),
            Engine::Postgres => (
                "SELECT ARRAY[1,2]::integer[], '(1,2)'::point, '{\"k\":true}'::jsonb, 9223372036854775807::bigint, decode('00ff','hex')",
                vec![
                    Value::Text("{1,2}".into()),
                    Value::Text("(1,2)".into()),
                    Value::Text("{\"k\": true}".into()),
                    Value::Text(i64::MAX.to_string()),
                    Value::Text("\\x00ff".into()),
                ],
            ),
            Engine::Mysql => (
                "SELECT CAST(18446744073709551615 AS UNSIGNED), CAST(1.25 AS DOUBLE), CAST(12345.67890 AS DECIMAL(20,5)), CAST('2026-10-16' AS DATE), X'00ff', NULL",
                vec![
                    Value::Text(u64::MAX.to_string()),
                    Value::Real("1.25".into()),
                    Value::Text("12345.67890".into()),
                    Value::Text("2026-10-16".into()),
                    Value::Blob(vec![0, 255]),
                    Value::Null,
                ],
            ),
        };
        assert_eq!(
            rows(execute(&client, query(&profile, sql)).await).rows,
            vec![expected]
        );
        let empty = rows(
            execute(
                &client,
                query(
                    &profile,
                    &format!("SELECT id, value FROM {table} WHERE 1=0"),
                ),
            )
            .await,
        );
        assert_eq!(empty.columns, ["id", "value"]);
        assert!(empty.rows.is_empty());
        assert!(!empty.truncated);
        for sql in ["SELECT 1; SELECT 2", "not SQL"] {
            assert_eq!(
                client
                    .execute(client.prepare(query(&profile, sql)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
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
        if matches!(connection, Connection::Sqlite { .. }) {
            checks::cancels_writes(&client, &profile, &table).await;
        }
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
    let path = directory.path().join("nested/fixture.sqlite3");
    exercise(
        Connection::Sqlite {
            path: path.to_str().unwrap().into(),
        },
        None,
    )
    .await;
}

#[tokio::test]
async fn sqlite_initializes_explicit_schema_writes() {
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
        let path = directory.path().join("output/data/created.db");
        let Output::DatabaseProfile(profile) = execute(
            &client,
            Command::SaveDatabase {
                profile: Profile {
                    id: DatabaseId::new(),
                    revision: 0,
                    sharing: None,
                    name: "Created SQLite fixture".into(),
                    connection: Connection::Sqlite {
                        path: path.to_str().unwrap().into(),
                    },
                    read_only: false,
                },
                expected_revision: 0,
                password: None,
            },
        )
        .await
        else {
            panic!("database profile expected")
        };
        assert!(!path.exists());
        assert!(!path.parent().unwrap().exists());
        let mut stale = client.prepare(query(&profile, "CREATE TABLE stale(value INTEGER)"));
        if let Command::QueryDatabase {
            expected_revision, ..
        } = &mut stale.command
        {
            *expected_revision = 0;
        }
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert!(!path.exists());
        assert!(!path.parent().unwrap().exists());
        let request = client.prepare(query(&profile, "CREATE TABLE items(value INTEGER)"));
        let created = client.execute(request.clone()).await.unwrap();
        assert!(path.is_file());
        assert_eq!(client.execute(request).await.unwrap(), created);
        execute(&client, query(&profile, "INSERT INTO items VALUES (42)")).await;
        assert_eq!(
            rows(execute(&client, query(&profile, "SELECT value FROM items")).await).rows,
            vec![vec![Value::Integer(42)]]
        );
        execute(
            &client,
            query(&profile, "CREATE TABLE other(value INTEGER)"),
        )
        .await;
        assert_eq!(
            rows(execute(&client, query(&profile, "SELECT value FROM items")).await).rows,
            vec![vec![Value::Integer(42)]]
        );
        let invalid = directory.path().join("invalid.db");
        let contents = b"Not a SQLite database";
        std::fs::write(&invalid, contents).unwrap();
        let Output::DatabaseProfile(invalid_profile) = execute(
            &client,
            Command::SaveDatabase {
                profile: Profile {
                    id: DatabaseId::new(),
                    revision: 0,
                    connection: Connection::Sqlite {
                        path: invalid.to_str().unwrap().into(),
                    },
                    ..profile.clone()
                },
                expected_revision: 0,
                password: None,
            },
        )
        .await
        else {
            panic!("database profile expected")
        };
        assert_eq!(
            client
                .execute(
                    client.prepare(query(&invalid_profile, "CREATE TABLE items(value INTEGER)"))
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(std::fs::read(&invalid).unwrap(), contents.to_vec());
        for target in [
            node.profile().join("created.db"),
            node.profile().join("absent/created.db"),
        ] {
            let Output::DatabaseProfile(profile) = execute(
                &client,
                Command::SaveDatabase {
                    profile: Profile {
                        id: DatabaseId::new(),
                        revision: 0,
                        connection: Connection::Sqlite {
                            path: target.to_str().unwrap().into(),
                        },
                        ..profile.clone()
                    },
                    expected_revision: 0,
                    password: None,
                },
            )
            .await
            else {
                panic!("database profile expected")
            };
            assert_eq!(
                client
                    .execute(client.prepare(query(&profile, "CREATE TABLE items(value INTEGER)")))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
            assert!(!target.exists());
            if target.parent().unwrap() != node.profile() {
                assert!(!target.parent().unwrap().exists());
            }
        }
        #[cfg(unix)]
        {
            let target = directory.path().join("unintended.db");
            let link = directory.path().join("dangling.db");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            let Output::DatabaseProfile(profile) = execute(
                &client,
                Command::SaveDatabase {
                    profile: Profile {
                        id: DatabaseId::new(),
                        revision: 0,
                        connection: Connection::Sqlite {
                            path: link.to_str().unwrap().into(),
                        },
                        ..profile.clone()
                    },
                    expected_revision: 0,
                    password: None,
                },
            )
            .await
            else {
                panic!("database profile expected")
            };
            assert_eq!(
                client
                    .execute(client.prepare(query(&profile, "CREATE TABLE items(value INTEGER)")))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
            assert!(!target.exists());
            assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn sqlite_catalog_reports_missing_without_creation() {
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
        let missing = directory.path().join("absent/missing.db");
        for read_only in [true, false] {
            let Output::DatabaseProfile(profile) = execute(
                &client,
                Command::SaveDatabase {
                    profile: Profile {
                        id: DatabaseId::new(),
                        revision: 0,
                        sharing: None,
                        name: "Missing SQLite fixture".into(),
                        connection: Connection::Sqlite {
                            path: missing.to_str().unwrap().into(),
                        },
                        read_only,
                    },
                    expected_revision: 0,
                    password: None,
                },
            )
            .await
            else {
                panic!("database profile expected")
            };
            for command in [
                Command::BrowseDatabase {
                    profile: profile.id,
                    expected_revision: profile.revision,
                    database: None,
                },
                query(&profile, "SELECT 1"),
                query(&profile, "EXPLAIN CREATE TABLE items(value INTEGER)"),
                query(&profile, "PRAGMA user_version = 7"),
                query(&profile, "EXPLAIN PRAGMA user_version = 7"),
                Command::QueryDatabase {
                    profile: profile.id,
                    expected_revision: profile.revision,
                    database: None,
                    sql: "CREATE TABLE items(value INTEGER)".into(),
                    row_limit: 2,
                    timeout_ms: 5000,
                    read_only: true,
                },
            ] {
                let fault = client.execute(client.prepare(command)).await.unwrap_err();
                assert_eq!(fault.code, ErrorCode::NotFound);
                assert_eq!(fault.message, "Database file does not exist");
                assert!(!missing.exists());
                assert!(!missing.parent().unwrap().exists());
            }
            if read_only {
                let fault = client
                    .execute(client.prepare(query(&profile, "CREATE TABLE items(value INTEGER)")))
                    .await
                    .unwrap_err();
                assert_eq!(fault.code, ErrorCode::NotFound);
                assert!(!missing.exists());
            } else {
                for sql in ["not SQL", "CREATE TABLE items(value INTEGER); SELECT 1"] {
                    let fault = client
                        .execute(client.prepare(query(&profile, sql)))
                        .await
                        .unwrap_err();
                    assert_eq!(fault.code, ErrorCode::InvalidRequest);
                    assert!(!missing.exists());
                }
            }
            let commands = [
                Command::CheckDatabase {
                    profile: profile.id,
                    expected_revision: profile.revision,
                },
                Command::TestDatabase {
                    profile: profile.clone(),
                    expected_revision: profile.revision,
                    password: None,
                },
            ];
            for command in commands {
                let result = client.execute(client.prepare(command)).await;
                if read_only {
                    assert_eq!(result.unwrap_err().code, ErrorCode::NotFound);
                    assert!(!missing.exists());
                    assert!(!missing.parent().unwrap().exists());
                } else {
                    assert_eq!(result.unwrap(), Output::DatabaseOutcome(Outcome::Connected));
                    assert!(missing.is_file());
                }
            }
            if !read_only {
                let draft_path = directory.path().join("draft/nested.db");
                let mut draft = profile.clone();
                draft.connection = Connection::Sqlite {
                    path: draft_path.to_str().unwrap().into(),
                };
                assert_eq!(
                    execute(
                        &client,
                        Command::TestDatabase {
                            profile: draft,
                            expected_revision: profile.revision,
                            password: None,
                        },
                    )
                    .await,
                    Output::DatabaseOutcome(Outcome::Connected)
                );
                assert!(draft_path.is_file());
                execute(
                    &client,
                    query(&profile, "CREATE TABLE items(value INTEGER)"),
                )
                .await;
                execute(&client, query(&profile, "INSERT INTO items VALUES (37)")).await;
                execute(
                    &client,
                    Command::CheckDatabase {
                        profile: profile.id,
                        expected_revision: profile.revision,
                    },
                )
                .await;
                assert_eq!(
                    rows(execute(&client, query(&profile, "SELECT value FROM items")).await).rows,
                    vec![vec![Value::Integer(37)]]
                );
            }
        }
        let empty = directory.path().join("empty.db");
        rusqlite::Connection::open(&empty).unwrap();
        let Output::DatabaseProfile(profile) = execute(
            &client,
            Command::SaveDatabase {
                profile: Profile {
                    id: DatabaseId::new(),
                    revision: 0,
                    sharing: None,
                    name: "Empty SQLite fixture".into(),
                    connection: Connection::Sqlite {
                        path: empty.to_str().unwrap().into(),
                    },
                    read_only: true,
                },
                expected_revision: 0,
                password: None,
            },
        )
        .await
        else {
            panic!("database profile expected")
        };
        assert_eq!(
            execute(
                &client,
                Command::BrowseDatabase {
                    profile: profile.id,
                    expected_revision: profile.revision,
                    database: None
                }
            )
            .await,
            Output::DatabaseOutcome(Outcome::Catalog(database::Catalog::Databases(vec![
                "main".into()
            ])))
        );
        assert_eq!(
            execute(
                &client,
                Command::BrowseDatabase {
                    profile: profile.id,
                    expected_revision: profile.revision,
                    database: Some("main".into())
                }
            )
            .await,
            Output::DatabaseOutcome(Outcome::Catalog(database::Catalog::Tables {
                database: "main".into(),
                tables: vec![]
            }))
        );
        let Output::DatabaseProfile(protected) = execute(
            &client,
            Command::SaveDatabase {
                profile: Profile {
                    connection: Connection::Sqlite {
                        path: node
                            .profile()
                            .join("storage/node.sqlite3")
                            .to_str()
                            .unwrap()
                            .into(),
                    },
                    ..profile.clone()
                },
                expected_revision: profile.revision,
                password: None,
            },
        )
        .await
        else {
            panic!("database profile expected")
        };
        let fault = client
            .execute(client.prepare(Command::BrowseDatabase {
                profile: protected.id,
                expected_revision: protected.revision,
                database: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(fault.code, ErrorCode::PermissionDenied);
        assert_eq!(
            fault.message,
            "Node storage cannot be opened as a query database"
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
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
                let error = client.execute(client.prepare(command)).await.unwrap_err();
                assert_eq!(error.code, ErrorCode::Unavailable);
                assert!(!error.message.contains(&password));
            }
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

async fn timeout(client: &Client, profile: &Profile) {
    let mut command = query(
        profile,
        if profile.connection.engine() == Engine::Mysql {
            "SELECT SLEEP(2)"
        } else {
            "SELECT pg_sleep(2)"
        },
    );
    if let Command::QueryDatabase { timeout_ms, .. } = &mut command {
        *timeout_ms = 50;
    }
    let error = client.execute(client.prepare(command)).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::OutcomeUnknown);
    assert_eq!(
        rows(execute(client, query(profile, "SELECT 1")).await)
            .rows
            .len(),
        1
    );
}

#[tokio::test]
#[ignore = "requires initdb, postgres, and mysqld binaries; creates isolated local servers"]
async fn isolated_engines() {
    let certificates = tls::Certificates::new();
    for engine in [Engine::Postgres, Engine::Mysql] {
        let server =
            servers::Server::start_with_tls(engine, Some(certificates.directory.path())).await;
        tls::verify(&server, &certificates).await;
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
        exercise_transport(
            server.socket(),
            Some(Secret::new(servers::PASSWORD.into())),
            None,
            false,
        )
        .await;
        let tunnel = tunnel::Server::start(&server.connection).await;
        exercise_transport(
            server.connection.clone(),
            Some(Secret::new(servers::PASSWORD.into())),
            Some(&tunnel),
            false,
        )
        .await;
    }
}
