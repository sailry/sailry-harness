use super::*;
use sailry_link::{Link, NetworkScope};
use sailry_protocol::conversation::{ModelApi, Provider};

#[tokio::test]
async fn restores_private_readback() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(node.local());
        let provider = Provider {
            id: ProviderId::new(),
            revision: 0,
            name: "Readback fixture".into(),
            api: ModelApi::ChatCompletions,
            authentication: Authentication::ApiKey,
            endpoint: "https://fixture.invalid/v1".into(),
            options: None,
            enabled: true,
            models: Vec::new(),
            default_model: "fixture".into(),
            credential: None,
        };
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::SaveProvider {
                provider,
                expected_revision: 0,
                secret: None,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let empty = client.prepare(Command::ReadProviderKey {
            provider: provider.id,
            expected_revision: provider.revision,
        });
        assert_eq!(
            client.execute(empty.clone()).await.unwrap(),
            Output::ProviderKey(None)
        );
        let marker = "isolated-provider-readback-secret";
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::SaveProvider {
                expected_revision: provider.revision,
                provider,
                secret: Some(Secret::new(marker.into())),
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        assert_eq!(
            client.execute(empty.clone()).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        let body: Vec<u8> = db
            .query_row(
                "SELECT body FROM providers WHERE id=?1",
                [provider.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let stored: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = provider.credential.as_ref().unwrap().id.to_string();
        assert_eq!(stored["authorizations"][&id]["key"], marker);
        assert_eq!(stored["provider"], serde_json::to_value(&provider).unwrap());
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='credentials'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
        drop(db);
        drop(client);
        node.shutdown().await.unwrap();

        let node = Node::start(&profile).await.unwrap();
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
        let read = client.prepare(Command::ReadProviderKey {
            provider: provider.id,
            expected_revision: provider.revision,
        });
        assert!(!read.command.durable());
        let output = client.execute(read.clone()).await.unwrap();
        assert_eq!(
            output,
            Output::ProviderKey(Some(Secret::new(marker.into())))
        );
        assert!(!format!("{output:?}").contains(marker));
        let plugin_read = read.clone().with_plugin(plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: plugin::Reference {
                name: "provider-fixture".into(),
                digest: "fixture".into(),
                settings_revision: 0,
            },
            worktree: None,
            session: None,
        });
        assert_eq!(
            client.execute(plugin_read).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        for command in [
            Command::Snapshot,
            Command::ListProviders,
            Command::ListCredentials,
        ] {
            let output = client.execute(client.prepare(command)).await.unwrap();
            assert!(!serde_json::to_string(&output).unwrap().contains(marker));
        }
        client
            .execute(client.prepare(Command::RevokeCredential {
                id: provider.credential.as_ref().unwrap().id,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        assert_eq!(
            client.execute(read.clone()).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        let mut cleared = provider;
        cleared.credential = None;
        let Output::Provider(cleared) = client
            .execute(client.prepare(Command::SaveProvider {
                expected_revision: cleared.revision,
                provider: cleared,
                secret: None,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadProviderKey {
                    provider: cleared.id,
                    expected_revision: cleared.revision,
                }))
                .await
                .unwrap(),
            Output::ProviderKey(None)
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();

        let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        for id in [empty.id, read.id] {
            let count: i64 = db
                .query_row(
                    "SELECT count(*) FROM requests WHERE id=?1",
                    [id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "private reads must not enter the durable ledger");
        }
        for query in [
            "SELECT body FROM requests",
            "SELECT CAST(result AS BLOB) FROM requests WHERE result IS NOT NULL",
            "SELECT body FROM events",
            "SELECT body FROM providers",
        ] {
            let rows = db
                .prepare(query)
                .unwrap()
                .query_map([], |row| row.get::<_, Vec<u8>>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(
                rows.iter()
                    .all(|bytes| !String::from_utf8_lossy(bytes).contains(marker))
            );
        }
    }
}
