use super::*;
use crate::store::database::Database;

#[test]
fn preserves_definitions_without_derived_database() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let path = root.join("node.sqlite3");
    let derived = root.join("knowledge.sqlite3");
    let source = root.join("reference.txt");
    std::fs::write(&source, "Original reference").unwrap();
    let node = NodeId([31; 32]);
    let db = Database::open(&path, node, None).unwrap();
    let selected = definition(Source::File {
        path: source.to_str().unwrap().into(),
    });
    let collection = create(
        &db.connection,
        &request(Command::CreateKnowledgeCollection {
            definition: selected,
            plugin: None,
        }),
    );
    execute(
        &db.connection,
        &request(Command::SaveKnowledgeSettings(knowledge::Settings {
            model: None,
            mirror: None,
            revision: 0,
        })),
    )
    .unwrap();
    db.connection
        .execute(
            "INSERT INTO plugin_values(name,key,revision,value) VALUES('owner','original',9,?1)",
            [br#"{"private":"untouched"}"#.as_slice()],
        )
        .unwrap();
    let metadata = raw(&db.connection);
    db.close().unwrap();
    Connection::open(&derived)
        .unwrap()
        .execute_batch("CREATE TABLE disposable(value TEXT)")
        .unwrap();
    std::fs::remove_file(&derived).unwrap();
    std::fs::remove_file(&source).unwrap();
    let db = Database::open(&path, node, None).unwrap();
    assert_eq!(raw(&db.connection), metadata);
    assert_eq!(catalog(&db.connection).unwrap().settings.model, None);
    let original: (i64, Vec<u8>) = db
        .connection
        .query_row(
            "SELECT revision,value FROM plugin_values WHERE name='owner' AND key='original'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(original, (9, br#"{"private":"untouched"}"#.to_vec()));
    let current = catalog(&db.connection)
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
    );
    assert_eq!(rebuilt.progress.generation, 3);
    execute(
        &db.connection,
        &request(Command::DeleteKnowledgeCollection {
            id: rebuilt.id,
            expected_revision: rebuilt.revision,
        }),
    )
    .unwrap();
    assert!(catalog(&db.connection).unwrap().collections.is_empty());
    db.close().unwrap();
}

#[test]
fn adds_missing_tables_with_original_backup() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let path = root.join("node.sqlite3");
    let node = NodeId([32; 32]);
    let db = Database::open(&path, node, None).unwrap();
    db.connection
        .execute(
            "INSERT INTO plugin_values(name,key,revision,value) VALUES('owner','original',7,?1)",
            [b"true".as_slice()],
        )
        .unwrap();
    db.connection
        .execute_batch("DROP TABLE knowledge_settings; DROP TABLE knowledge_collections;")
        .unwrap();
    db.close().unwrap();
    let db = Database::open(&path, node, None).unwrap();
    assert_eq!(
        catalog(&db.connection).unwrap().settings,
        knowledge::Settings::default()
    );
    assert!(catalog(&db.connection).unwrap().collections.is_empty());
    let original: (i64, Vec<u8>) = db
        .connection
        .query_row(
            "SELECT revision,value FROM plugin_values WHERE name='owner' AND key='original'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(original, (7, b"true".to_vec()));
    assert_eq!(
        db.connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    db.close().unwrap();
    let backups: Vec<_> = std::fs::read_dir(root.join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1);
    let saved =
        Connection::open_with_flags(&backups[0], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    assert!(
        saved
            .prepare("SELECT body FROM knowledge_settings")
            .is_err()
    );
    let original: (i64, Vec<u8>) = saved
        .query_row(
            "SELECT revision,value FROM plugin_values WHERE name='owner' AND key='original'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(original, (7, b"true".to_vec()));
}

#[test]
fn malformed_configuration_preserves_original_profile() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let path = root.join("node.sqlite3");
    let node = NodeId([33; 32]);
    let db = Database::open(&path, node, None).unwrap();
    db.connection
        .execute(
            "INSERT INTO knowledge_settings(singleton,body) VALUES(1,?1)",
            [br#"{"private-marker""#.as_slice()],
        )
        .unwrap();
    db.connection
        .execute_batch("DROP TABLE knowledge_collections;")
        .unwrap();
    db.close().unwrap();
    let original = std::fs::read(&path).unwrap();
    let error = Database::open(&path, node, None).err().unwrap().to_string();
    assert!(
        error.contains("invalid stored JSON in knowledge_settings.body"),
        "{error}"
    );
    assert!(!error.contains("private-marker"), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(!root.join("backups").exists());
}
