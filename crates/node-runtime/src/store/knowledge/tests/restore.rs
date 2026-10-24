use super::*;
use crate::store::database::Database;

#[tokio::test]
async fn preserves_definitions_without_derived_database() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let path = root.join("node.turso");
    let derived = root.join("knowledge.turso");
    let source = root.join("reference.txt");
    std::fs::write(&source, "Original reference").unwrap();
    let node = NodeId([31; 32]);
    let db = Database::open(&path, node, None).await.unwrap();
    let selected = definition(Source::File {
        path: source.to_str().unwrap().into(),
    });
    let collection = create(
        &db.connection,
        &request(Command::CreateKnowledgeCollection {
            definition: selected,
            plugin: None,
        }),
    )
    .await;
    let mut settings = sailry_protocol::models::Settings::default();
    select_model(&mut settings, None);
    execute(
        &db.connection,
        &request(Command::SaveLocalModelSettings(settings)),
    )
    .await
    .unwrap();
    db.connection
        .execute(
            "INSERT INTO plugin_values(name,key,revision,value) VALUES('owner','original',9,?1)",
            [br#"{"private":"untouched"}"#.as_slice()],
        )
        .await
        .unwrap();
    let metadata = raw(&db.connection).await;
    db.close().await.unwrap();
    let derived_handle = crate::store::database::engine(&derived).await.unwrap();
    let derived_connection = crate::store::database::connect(&derived_handle)
        .await
        .unwrap();
    derived_connection
        .execute_batch("CREATE TABLE disposable(value TEXT)")
        .await
        .unwrap();
    drop(derived_connection);
    drop(derived_handle);
    for (index, file) in crate::profile::database_files(&derived)
        .into_iter()
        .enumerate()
    {
        match std::fs::remove_file(file) {
            Err(error) if index >= 2 && error.kind() == std::io::ErrorKind::NotFound => {}
            result => result.unwrap(),
        }
    }
    std::fs::remove_file(&source).unwrap();
    let db = Database::open(&path, node, None).await.unwrap();
    assert_eq!(raw(&db.connection).await, metadata);
    assert_eq!(
        local_models::settings(&db.connection)
            .await
            .unwrap()
            .choice(sailry_protocol::models::Kind::Embedding)
            .model,
        None
    );
    let original: (i64, Vec<u8>) = (async {
        let mut statement = db
            .connection
            .prepare(
                "SELECT revision,value FROM plugin_values WHERE name='owner' AND key='original'",
            )
            .await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
        mapped
    })
    .await
    .unwrap();
    assert_eq!(original, (9, br#"{"private":"untouched"}"#.to_vec()));
    let current = catalog(&db.connection)
        .await
        .unwrap()
        .collections
        .remove(0)
        .collection;
    assert_eq!(current.id, collection.id);
    assert_eq!(current.progress.generation, 2);
    let rebuilt = create(
        &db.connection,
        &request(Command::RebuildKnowledgeCollection {
            id: current.id,
            expected_revision: current.revision,
        }),
    )
    .await;
    assert_eq!(rebuilt.progress.generation, 3);
    execute(
        &db.connection,
        &request(Command::DeleteKnowledgeCollection {
            id: rebuilt.id,
            expected_revision: rebuilt.revision,
        }),
    )
    .await
    .unwrap();
    assert!(
        catalog(&db.connection)
            .await
            .unwrap()
            .collections
            .is_empty()
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn adds_missing_tables_with_original_backup() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let path = root.join("node.turso");
    let node = NodeId([32; 32]);
    let db = Database::open(&path, node, None).await.unwrap();
    db.connection
        .execute(
            "INSERT INTO plugin_values(name,key,revision,value) VALUES('owner','original',7,?1)",
            [b"true".as_slice()],
        )
        .await
        .unwrap();
    db.connection
        .execute_batch("DROP TABLE local_model_settings; DROP TABLE knowledge_collections;")
        .await
        .unwrap();
    db.close().await.unwrap();
    let db = Database::open(&path, node, None).await.unwrap();
    assert_eq!(
        local_models::settings(&db.connection).await.unwrap(),
        sailry_protocol::models::Settings::default()
    );
    assert!(
        catalog(&db.connection)
            .await
            .unwrap()
            .collections
            .is_empty()
    );
    let original: (i64, Vec<u8>) = (async {
        let mut statement = db
            .connection
            .prepare(
                "SELECT revision,value FROM plugin_values WHERE name='owner' AND key='original'",
            )
            .await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
        mapped
    })
    .await
    .unwrap();
    assert_eq!(original, (7, b"true".to_vec()));
    assert_eq!(
        db.connection
            .prepare("PRAGMA user_version")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        1
    );
    db.close().await.unwrap();
    let backups: Vec<_> = std::fs::read_dir(root.join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1);
    let saved_handle = turso::Builder::new_local(backups[0].join("node.turso").to_str().unwrap())
        .experimental_index_method(true)
        .read_only(true)
        .build()
        .await
        .unwrap();
    let saved = crate::store::database::connect(&saved_handle)
        .await
        .unwrap();
    assert!(
        saved
            .prepare("SELECT body FROM local_model_settings")
            .await
            .is_err()
    );
    let original: (i64, Vec<u8>) = (async {
        let mut statement = saved
            .prepare(
                "SELECT revision,value FROM plugin_values WHERE name='owner' AND key='original'",
            )
            .await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
        mapped
    })
    .await
    .unwrap();
    assert_eq!(original, (7, b"true".to_vec()));
}

#[tokio::test]
async fn malformed_configuration_preserves_original_profile() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let path = root.join("node.turso");
    let node = NodeId([33; 32]);
    let db = Database::open(&path, node, None).await.unwrap();
    db.connection
        .execute(
            "UPDATE local_model_settings SET body=?1 WHERE singleton=1",
            [br#"{"private-marker""#.as_slice()],
        )
        .await
        .unwrap();
    db.connection
        .execute_batch("DROP TABLE knowledge_collections;")
        .await
        .unwrap();
    db.close().await.unwrap();
    let original = family(&path);
    let error = Database::open(&path, node, None)
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("invalid stored JSON in local_model_settings.body"),
        "{error}"
    );
    assert!(!error.contains("private-marker"), "{error}");
    assert_eq!(family(&path), original);
    assert!(!root.join("backups").exists());
}

fn family(path: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    // The main file and MVCC log are required; the remaining sidecars are optional.
    crate::profile::database_files(path)
        .into_iter()
        .enumerate()
        .filter_map(|(index, file)| match std::fs::read(&file) {
            Ok(bytes) => Some((
                file.file_name().unwrap().to_string_lossy().into_owned(),
                bytes,
            )),
            Err(error) if index >= 2 && error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("fixture database family read failed: {error}"),
        })
        .collect()
}
