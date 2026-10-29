use super::*;

#[tokio::test]
async fn failed_cache_cleanup_preserves_reset_marker() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node.turso");
    let knowledge = directory.path().join("knowledge.turso");
    let node = NodeId([90; 32]);
    {
        let (handle, connection) = empty(&path, node, None).await.unwrap();
        connection
            .execute_batch(
                "ALTER TABLE node DROP COLUMN storage_generation;
                 INSERT INTO counters(namespace,value) VALUES('sessions',73)",
            )
            .await
            .unwrap();
        driver::checkpoint(&connection).await.unwrap();
        drop(connection);
        drop(handle);
    }
    std::fs::create_dir(&knowledge).unwrap();
    assert!(matches!(
        upgrade(&path, node, None, Source::Profile).await,
        Err(Error::Io(_))
    ));
    assert!(path.is_file());
    {
        let handle = engine(&path).await.unwrap();
        let connection = connect(&handle).await.unwrap();
        let value = connection
            .prepare("SELECT value FROM counters WHERE namespace='sessions'")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<i64>(0)
            .unwrap();
        assert_eq!(value, 73);
        assert!(
            schema::prepare(&connection, &path, node, None)
                .await
                .unwrap()
                .needs_rebuild()
        );
    }
    std::fs::remove_dir(&knowledge).unwrap();
    let (_handle, connection, backup) = upgrade(&path, node, None, Source::Profile).await.unwrap();
    assert!(backup.is_none());
    assert!(!knowledge.exists());
    let count = connection
        .prepare("SELECT count(*) FROM counters WHERE namespace='sessions'")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get::<i64>(0)
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn failed_rehearsal_preserves_live_packages() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("profile");
    let path = profile.join("storage/node.turso");
    crate::profile::private_directory(path.parent().unwrap()).unwrap();
    let node = NodeId([89; 32]);
    let database = crate::store::database::Database::open(&path, node, Some(profile.clone()))
        .await
        .unwrap();
    let request = sailry_protocol::Request::new(
        node,
        sailry_protocol::Command::SaveCodeIndexSettings {
            settings: Default::default(),
            expected_revision: 0,
        },
    );
    database.connection.execute(
        "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed','invalid')",
        turso::params![&node.0[..], request.id.to_string(), serde_json::to_vec(&request).unwrap()],
    ).await.unwrap();
    database
        .connection
        .execute("DROP TABLE code_index_settings", ())
        .await
        .unwrap();
    database.close().await.unwrap();
    let host = crate::plugins::Host::new(Some(profile.clone()));
    assert!(
        crate::store::database::Database::open_with_plugins(
            &path,
            node,
            Some(profile.clone()),
            Some(&host)
        )
        .await
        .is_err()
    );
    assert!(!profile.join("plugins/packages").exists());
    assert!(!profile.join("storage/backups").exists());
    let handle = engine(&path).await.unwrap();
    let connection = connect(&handle).await.unwrap();
    let exists: bool = connection
        .prepare("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='code_index_settings')")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert!(!exists);
    let result: String = connection
        .prepare("SELECT result FROM requests WHERE id=?1")
        .await
        .unwrap()
        .query_row([request.id.to_string()])
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(result, "invalid");
}
