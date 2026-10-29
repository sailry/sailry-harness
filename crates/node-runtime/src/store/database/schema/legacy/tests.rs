use super::*;

async fn fixture(sql: &str) -> (tempfile::TempDir, turso::Database, Connection) {
    let directory = tempfile::tempdir().unwrap();
    let database = crate::store::database::engine(&directory.path().join("node.turso"))
        .await
        .unwrap();
    let connection = crate::store::database::connect(&database).await.unwrap();
    connection.execute_batch(sql).await.unwrap();
    (directory, database, connection)
}

#[tokio::test]
async fn recognizes_explicit_markers() {
    for sql in [
        "CREATE TABLE agent_events(body BLOB); INSERT INTO agent_events VALUES('{}')",
        "CREATE TABLE search_settings(body BLOB); INSERT INTO search_settings VALUES('{\"settings\":{\"prefer_native\":false}}')",
        "CREATE TABLE search_settings(body BLOB); INSERT INTO search_settings VALUES('{\"settings\":{\"prefer_native\":null}}')",
        "CREATE TABLE roles(body BLOB); INSERT INTO roles VALUES('{\"tools\":[{\"kind\":\"model\",\"name\":\"web_search\"}]}')",
    ] {
        let (_directory, _database, connection) = fixture(sql).await;
        assert!(detected(&connection).await.unwrap(), "{sql}");
    }
    for index in INDEXES {
        let sql = format!(
            "CREATE TABLE agent_events(turn TEXT,sequence INTEGER,body BLOB);{}",
            index.to_lowercase()
        );
        let (_directory, _database, connection) = fixture(&sql).await;
        assert!(detected(&connection).await.unwrap(), "{sql}");
    }
}

#[tokio::test]
async fn leaves_current_and_unrelated_shapes_alone() {
    for sql in [
        "",
        "CREATE TABLE agent_events(body BLOB); INSERT INTO agent_events VALUES('{\"kind\":\"message\"}')",
        "CREATE TABLE agent_events(body BLOB); INSERT INTO agent_events VALUES('{\"kind\":null}')",
        "CREATE TABLE search_settings(body BLOB); INSERT INTO search_settings VALUES('{\"settings\":{},\"prefer_native\":true}')",
        "CREATE TABLE roles(body BLOB); INSERT INTO roles VALUES('{\"tools\":[{\"kind\":\"builtin\",\"name\":\"web_search\"}]}')",
        "CREATE TABLE agent_events(body BLOB); CREATE INDEX other_usage ON agent_events(json_extract(body,'$.usage_metadata'))",
        "CREATE TABLE foreign_events(body BLOB); CREATE INDEX agent_usage ON foreign_events(json_extract(body,'$.usage_metadata'))",
        "CREATE TABLE agent_events(body BLOB); CREATE INDEX agent_usage ON agent_events(json_extract(body,'$.metadata.usage.input'))",
        "CREATE TABLE agent_events(body BLOB); CREATE INDEX agent_usage ON agent_events(json_extract(body,'$.usage_metadata_other'))",
        "CREATE TABLE agent_events(body BLOB); CREATE INDEX agent_usage ON agent_events(body) WHERE body != '$.usage_metadata'",
        "CREATE TABLE agent_events(body BLOB); CREATE INDEX agent_usage ON agent_events(body) WHERE body != 'JSON_EXTRACT(BODY,''$.usage_metadata'')'",
    ] {
        let (_directory, _database, connection) = fixture(sql).await;
        assert!(!detected(&connection).await.unwrap(), "{sql}");
    }
}

#[tokio::test]
async fn rejects_invalid_json_without_reclassification() {
    for table in ["agent_events", "search_settings", "roles"] {
        let (_directory, _database, connection) = fixture(&format!(
            "CREATE TABLE {table}(body BLOB); INSERT INTO {table} VALUES('invalid')"
        ))
        .await;
        assert!(detected(&connection).await.is_err(), "{table}");
    }
}
