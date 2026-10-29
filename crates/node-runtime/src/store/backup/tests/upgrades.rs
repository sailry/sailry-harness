use super::*;

#[tokio::test]
async fn rejects_unmarked_archives_on_both_paths() {
    use sailry_client::Client;
    use sailry_link::{Link, NetworkScope};

    let fixture = Fixture::new().await;
    let cancel = CancellationToken::new();
    let source = source(&fixture.storage, Some(&fixture.knowledge), 1, &cancel)
        .await
        .unwrap();
    let (_, mut snapshot, roots) = capture(source, &cancel).unwrap();
    snapshot.capture(&roots, &cancel).unwrap();
    let path = snapshot.directory.path().join("storage/node.turso");
    {
        let handle = Handle::new(
            path.clone(),
            super::super::super::database::engine(&path).await.unwrap(),
        );
        let connection = handle.lease(false).await.unwrap();
        connection
            .execute("ALTER TABLE node DROP COLUMN storage_generation", ())
            .await
            .unwrap();
        drop(connection);
        drop(handle.pause().await.unwrap());
    }
    snapshot
        .manifest
        .refresh_databases(snapshot.directory.path(), &cancel)
        .unwrap();
    let passphrase = Secret::new("isolated backup fixture passphrase".into());
    let packed =
        backup::encryption::seal(snapshot.pack(&cancel).unwrap(), &passphrase, &cancel).unwrap();
    let bytes = std::fs::read(packed.file.path()).unwrap();
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let profile = root.join("node");
        let node = crate::Node::start_empty(&profile).await.unwrap();
        let controller = Link::controller(root.join("controller"), NetworkScope::default())
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
        let Output::BackupSettings(saved) = client
            .execute(client.prepare(Command::SaveBackupSettings(Update {
                settings: Settings::default(),
                identity: SecretUpdate::Keep,
                secret: SecretUpdate::Keep,
                session_token: SecretUpdate::Keep,
                passphrase: SecretUpdate::Replace(passphrase.clone()),
            })))
            .await
            .unwrap()
        else {
            panic!("backup settings expected")
        };
        let Output::BackupUpload(upload) = client
            .execute(client.prepare(Command::UploadBackup {
                size: bytes.len() as u64,
                revision: blake3::hash(&bytes).to_hex().to_string(),
            }))
            .await
            .unwrap()
        else {
            panic!("backup upload expected")
        };
        client
            .upload(
                &upload,
                &mut std::io::Cursor::new(&bytes),
                cancel.clone(),
                |_| {},
            )
            .await
            .unwrap();
        let target = root.join("restored");
        let request = client.prepare(Command::RestoreBackupFile {
            stream: upload.stream,
            expected_revision: saved.revision,
            target: target.to_str().unwrap().into(),
            passphrase: None,
        });
        let error = client.execute(request.clone()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(error.message, "backup storage generation is incompatible");
        assert_eq!(client.execute(request).await.unwrap_err(), error);
        assert!(!target.exists());
        assert!(!profile.join("storage/backups").exists());
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadBackupSettings))
                .await
                .unwrap(),
            Output::BackupSettings(saved)
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
    fixture.close().await;
}

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
