use super::*;

async fn fixture(columns: &str, value: &str) -> (tempfile::TempDir, turso::Database, Connection) {
    let directory = tempfile::tempdir().unwrap();
    let database = crate::store::database::engine(&directory.path().join("node.turso"))
        .await
        .unwrap();
    let connection = crate::store::database::connect(&database).await.unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TABLE node(singleton INTEGER PRIMARY KEY{columns}); INSERT INTO node VALUES(1{value})"
        ))
        .await
        .unwrap();
    (directory, database, connection)
}

#[tokio::test]
async fn recognizes_missing_and_current() {
    for (columns, value, expected) in [
        ("", "", false),
        (",storage_generation INTEGER NOT NULL DEFAULT 1", ",1", true),
    ] {
        let (_directory, _database, connection) = fixture(columns, value).await;
        assert_eq!(
            current(&connection, Path::new("fixture")).await.unwrap(),
            expected
        );
    }
}

#[tokio::test]
async fn rejects_invalid_and_future_values() {
    for value in ["NULL", "0", "-1", "2", "'1'", "1.0", "X'01'"] {
        let (_directory, _database, connection) =
            fixture(",storage_generation", &format!(",{value}")).await;
        let before = connection
            .prepare("SELECT quote(storage_generation) FROM node")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<String>(0)
            .unwrap();
        let error = current(&connection, Path::new("fixture"))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("node.storage_generation"), "{error}");
        assert!(error.contains("existing data was not changed"), "{error}");
        assert_eq!(
            connection
                .prepare("SELECT quote(storage_generation) FROM node")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap()
                .get::<String>(0)
                .unwrap(),
            before
        );
    }
}
