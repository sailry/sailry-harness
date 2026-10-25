use super::*;
use crate::store::database::Database;

const SETTINGS: &str = "CREATE TABLE code_index_settings (singleton INTEGER PRIMARY KEY CHECK(singleton=1),body BLOB NOT NULL)";

#[tokio::test]
async fn preserves_enable_choice_and_original_backup() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node.turso");
    let node = NodeId([84; 32]);
    let database = Database::open(&path, node, None).await.unwrap();
    let old = serde_json::to_vec(&sailry_protocol::code::Settings {
        revision: 9,
        enabled: false,
        ..Default::default()
    })
    .unwrap();
    database
        .connection
        .execute(
            "INSERT INTO code_settings(singleton,body) VALUES(1,?1)",
            [old.as_slice()],
        )
        .await
        .unwrap();
    database
        .connection
        .execute("DROP TABLE code_index_settings", ())
        .await
        .unwrap();
    let before = objects(&database.connection).await.unwrap();
    database.close().await.unwrap();
    let database = Database::open(&path, node, None).await.unwrap();
    let settings = crate::store::code_index::settings(&database.connection)
        .await
        .unwrap();
    assert_eq!(settings.revision, 9);
    assert!(!settings.enabled);
    let kept: Vec<u8> = database
        .connection
        .prepare("SELECT body FROM code_settings WHERE singleton=1")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(kept, old);
    database.close().await.unwrap();
    let backups = std::fs::read_dir(directory.path().join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("node.turso"))
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    let backup = turso::Builder::new_local(backups[0].to_str().unwrap())
        .experimental_index_method(true)
        .read_only(true)
        .build()
        .await
        .unwrap();
    let connection = backup.connect().unwrap();
    assert_eq!(objects(&connection).await.unwrap(), before);
    let kept: Vec<u8> = connection
        .prepare("SELECT body FROM code_settings WHERE singleton=1")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(kept, old);
}

#[tokio::test]
async fn rejects_invalid_data_without_applying_changes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node.turso");
    let node = NodeId([85; 32]);
    let database = Database::open(&path, node, None).await.unwrap();
    database
        .connection
        .execute(
            "INSERT INTO code_settings(singleton,body) VALUES(1,?1)",
            [b"invalid".as_slice()],
        )
        .await
        .unwrap();
    database
        .connection
        .execute("DROP TABLE code_index_settings", ())
        .await
        .unwrap();
    let before = objects(&database.connection).await.unwrap();
    database.close().await.unwrap();
    assert!(Database::open(&path, node, None).await.is_err());
    let database = turso::Builder::new_local(path.to_str().unwrap())
        .experimental_index_method(true)
        .read_only(true)
        .build()
        .await
        .unwrap();
    let connection = database.connect().unwrap();
    assert_eq!(objects(&connection).await.unwrap(), before);
    let kept: Vec<u8> = connection
        .prepare("SELECT body FROM code_settings WHERE singleton=1")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(kept, b"invalid");
}

#[tokio::test]
async fn rollback_restores_absent_settings_table() {
    let ddl = DDL.replace(&format!("{SETTINGS};"), "");
    let (_directory, _database, connection) = fixture(&ddl).await;
    let before = objects(&connection).await.unwrap();
    let expected = expected(DDL, &[]).await.unwrap();
    let plan = plan::Plan::prepare(&connection, Path::new("fixture"), &expected, &[])
        .await
        .unwrap();
    assert!(plan.creates("code_index_settings"));
    connection.execute("BEGIN", ()).await.unwrap();
    plan.apply(&connection).await.unwrap();
    upgrades::apply(&connection).await.unwrap();
    verify_objects(&connection, Path::new("fixture"), &expected)
        .await
        .unwrap();
    connection.execute("ROLLBACK", ()).await.unwrap();
    assert_eq!(objects(&connection).await.unwrap(), before);
}
