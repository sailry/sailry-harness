use super::*;

#[tokio::test]
async fn rejects_password_reuse_after_target_changes() {
    for remote in [false, true] {
        let fixture = fixture::Fixture::new().await;
        let client = fixture.client(remote);
        let draft = Profile {
            id: DatabaseId::new(),
            revision: 0,
            sharing: None,
            name: "Private connection".into(),
            read_only: true,
            connection: Connection::Network {
                engine: Engine::Postgres,
                host: "database.test".into(),
                port: 5432,
                database: "project".into(),
                username: "reader".into(),
                tls: database::Tls::Require,
            },
        };
        let Output::DatabaseProfile(mut current) = execute(
            &client,
            Command::SaveDatabase {
                profile: draft,
                expected_revision: 0,
                password: Some(Secret::new("private-password".into())),
            },
        )
        .await
        else {
            panic!("database expected")
        };
        current.name = "Renamed".into();
        current.read_only = false;
        let Output::DatabaseProfile(current) = execute(
            &client,
            Command::SaveDatabase {
                expected_revision: current.revision,
                profile: current,
                password: None,
            },
        )
        .await
        else {
            panic!("database expected")
        };
        for field in ["host", "port", "username", "database", "tls", "engine"] {
            let mut changed = current.clone();
            let Connection::Network {
                host,
                port,
                username,
                database,
                tls,
                engine,
            } = &mut changed.connection
            else {
                unreachable!()
            };
            match field {
                "host" => *host = "other.test".into(),
                "port" => *port += 1,
                "username" => *username = "other".into(),
                "database" => *database = "other".into(),
                "tls" => *tls = database::Tls::Disable,
                "engine" => *engine = Engine::Mysql,
                _ => unreachable!(),
            }
            for command in [
                Command::SaveDatabase {
                    profile: changed.clone(),
                    expected_revision: current.revision,
                    password: None,
                },
                Command::TestDatabase {
                    profile: changed,
                    expected_revision: current.revision,
                    password: None,
                },
            ] {
                let error = client.execute(client.prepare(command)).await.unwrap_err();
                assert_eq!(
                    error.code,
                    ErrorCode::NotConfigured,
                    "{field}, remote={remote}"
                );
                assert!(!error.message.contains("private-password"));
            }
        }
        assert_eq!(
            execute(&client, Command::ListDatabases).await,
            Output::DatabaseProfiles(vec![current.clone()])
        );
        let mut changed = current.clone();
        if let Connection::Network { host, .. } = &mut changed.connection {
            *host = "replacement.test".into();
        }
        let Output::DatabaseProfile(saved) = execute(
            &client,
            Command::SaveDatabase {
                expected_revision: current.revision,
                profile: changed.clone(),
                password: Some(Secret::new("replacement-password".into())),
            },
        )
        .await
        else {
            panic!("database expected")
        };
        assert_eq!(saved.connection, changed.connection);
        fixture.close().await;
    }
}
