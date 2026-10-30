use super::*;
use sailry_protocol::Fault;

#[tokio::test]
async fn persists_order_and_rejects_invalid_updates() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let mut settings = read(&client).await;
        settings.order.reverse();
        let saved = save(&client, settings, Default::default()).await;
        for order in [
            Vec::new(),
            vec![Driver::Model; Driver::ALL.len()],
            Driver::ALL[..8].to_vec(),
        ] {
            let mut invalid = saved.clone();
            invalid.order = order;
            let error = client
                .execute(client.prepare(Command::SaveSearchSettings(search::Update {
                    settings: invalid,
                    credentials: Default::default(),
                })))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert_eq!(read(&client).await, saved);
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let (controller, transport) = super::controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        assert_eq!(read(&client).await, saved);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn adds_missing_order_with_private_backup() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let mut settings = read(&client).await;
        settings.fallback = false;
        let saved = save(
            &client,
            settings,
            BTreeMap::from([(
                Driver::Exa,
                search::Credentials {
                    key: SecretUpdate::Replace(Secret::new("order-upgrade-key".into())),
                    ..Default::default()
                },
            )]),
        )
        .await;
        node.with_storage(|db| Box::pin(async move {
            db.execute("UPDATE search_settings SET body=CAST(json_remove(body,'$.settings.order') AS BLOB)", ())
                .await.map_err(crate::database_fixture::fault)?;
            Ok::<_, Fault>(())
        })).await.unwrap();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let (controller, transport) = super::controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        assert_eq!(read(&client).await, saved);
        let backups = std::fs::read_dir(profile.join("storage/backups"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(backups.len(), 1);
        let (handle, db) =
            crate::database_fixture::open(&backups[0].path().join("node.turso"), true).await;
        let body: Vec<u8> = db
            .prepare("SELECT body FROM search_settings")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get(0)
            .unwrap();
        let original: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(original["settings"].get("order").is_none());
        assert_eq!(original["credentials"]["exa"]["key"], "order-upgrade-key");
        crate::database_fixture::close(handle, db).await;
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
