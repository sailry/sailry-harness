use super::*;

#[tokio::test]
async fn restores_missing_schema_without_running_recovery() {
    let fixture = Fixture::new().await;
    let cancel = CancellationToken::new();
    let source = source(&fixture.storage, Some(&fixture.knowledge), 1, &cancel)
        .await
        .unwrap();
    let (_, mut snapshot, roots) = capture(source, &cancel).unwrap();
    snapshot.capture(&roots, &cancel).unwrap();
    let path = snapshot.directory.path().join("storage/node.turso");
    let handle = Handle::new(
        path.clone(),
        super::super::super::database::engine(&path).await.unwrap(),
    );
    let connection = handle.lease(false).await.unwrap();
    connection
        .execute("DROP TABLE code_index_settings", ())
        .await
        .unwrap();
    let request = Request::new(
        fixture.storage.node,
        Command::SaveCodeIndexSettings {
            settings: Default::default(),
            expected_revision: 0,
        },
    );
    connection
        .execute(
            "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
            params![
                &fixture.storage.node.0[..],
                request.id.to_string(),
                serde_json::to_vec(&request).unwrap()
            ],
        )
        .await
        .unwrap();
    drop(connection);
    drop(handle.pause().await.unwrap());
    snapshot
        .manifest
        .refresh_databases(snapshot.directory.path(), &cancel)
        .unwrap();
    let packed = snapshot.pack(&cancel).unwrap();
    let record = backup::record(&packed, uuid::Uuid::new_v4().to_string()).unwrap();
    let target = fixture
        ._root
        .path()
        .canonicalize()
        .unwrap()
        .join("upgraded");
    publish(packed.file, &target, RecordSource::Known(record), &cancel)
        .await
        .unwrap();

    let path = target.join("storage/node.turso");
    let handle = Handle::new(
        path.clone(),
        super::super::super::database::engine(&path).await.unwrap(),
    );
    let connection = handle.lease(true).await.unwrap();
    let settings = crate::store::code_index::settings(&connection)
        .await
        .unwrap();
    assert!(settings.enabled);
    let status: String = connection
        .prepare("SELECT status FROM requests WHERE id=?1")
        .await
        .unwrap()
        .query_row([request.id.to_string()])
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(status, "admitted");
    let counter: i64 = connection
        .prepare("SELECT value FROM counters WHERE namespace='sessions'")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(counter, 73);
    drop(connection);
    drop(handle.pause().await.unwrap());

    let backups = std::fs::read_dir(target.join("storage/backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("node.turso"))
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    let path = &backups[0];
    let handle = Handle::new(
        path.clone(),
        super::super::super::database::engine(path).await.unwrap(),
    );
    let connection = handle.lease(true).await.unwrap();
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
    let status: String = connection
        .prepare("SELECT status FROM requests WHERE id=?1")
        .await
        .unwrap()
        .query_row([request.id.to_string()])
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(status, "admitted");
    drop(connection);
    drop(handle.pause().await.unwrap());
    fixture.close().await;
}
