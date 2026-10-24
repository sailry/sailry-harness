use super::*;
use crate::store::database::{
    Database, connect,
    counters::{self, Namespace},
    engine,
};
use sailry_protocol::NodeId;

async fn fixture() -> (tempfile::TempDir, turso::Database, Connection) {
    let directory = tempfile::tempdir().unwrap();
    let database = engine(&directory.path().join("node.turso")).await.unwrap();
    let connection = connect(&database).await.unwrap();
    connection
        .execute("CREATE TABLE records(body BLOB)", ())
        .await
        .unwrap();
    (directory, database, connection)
}

#[tokio::test]
async fn reports_locations_without_payloads() {
    let (_directory, _database, connection) = fixture().await;
    connection
        .execute(
            "INSERT INTO records VALUES(?1)",
            [br#"{"permission":"diagnostic-secret-marker"}"#.as_slice()],
        )
        .await
        .unwrap();
    let error = records::<SessionConfig>(&connection, "records.body", "SELECT body FROM records")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("records.body"), "{error}");
    assert!(error.contains("record 1"), "{error}");
    assert!(error.contains("line 1"), "{error}");
    assert!(!error.contains("diagnostic-secret-marker"), "{error}");
}

#[tokio::test]
async fn rejects_non_blob_records() {
    let (_directory, _database, connection) = fixture().await;
    connection
        .execute("INSERT INTO records VALUES('{}')", ())
        .await
        .unwrap();
    let error =
        records::<serde_json::Value>(&connection, "records.body", "SELECT body FROM records")
            .await
            .unwrap_err()
            .to_string();
    assert!(error.contains("invalid stored JSON type"), "{error}");
}

#[tokio::test]
async fn accepts_optional_defaults_without_rewriting_bytes() {
    let (_directory, _database, connection) = fixture().await;
    let before = br#"{"revision":1,"after_edit":true,"servers":{},"languages":{}}"#.as_slice();
    connection
        .execute("INSERT INTO records VALUES(?1)", [before])
        .await
        .unwrap();
    records::<sailry_protocol::code::Settings>(
        &connection,
        "records.body",
        "SELECT body FROM records",
    )
    .await
    .unwrap();
    let after = connection
        .prepare("SELECT body FROM records")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get::<Vec<u8>>(0)
        .unwrap();
    assert_eq!(after, before);
}

#[tokio::test]
async fn validates_archives_only_when_requested() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&directory.path().join("node.turso"), NodeId([53; 32]), None)
        .await
        .unwrap();
    database
        .connection
        .execute("BEGIN CONCURRENT", ())
        .await
        .unwrap();
    let cursor = counters::next(&database.connection, Namespace::Events)
        .await
        .unwrap();
    database
        .connection
        .execute(
            "INSERT INTO events(cursor,body) VALUES(?1,?2)",
            (
                cursor as i64,
                br#"{"kind":"diagnostic-secret-marker","data":null}"#.as_slice(),
            ),
        )
        .await
        .unwrap();
    database.connection.execute("COMMIT", ()).await.unwrap();
    startup(&database.connection).await.unwrap();
    let error = stored(&database.connection).await.unwrap_err().to_string();
    assert!(error.contains("events.body"), "{error}");
    assert!(!error.contains("diagnostic-secret-marker"), "{error}");
    database.close().await.unwrap();
}
