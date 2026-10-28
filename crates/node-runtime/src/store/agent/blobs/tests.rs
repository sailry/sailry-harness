use super::*;
use serde_json::json;

async fn database(directory: &tempfile::TempDir) -> (turso::Database, Connection) {
    let database = crate::store::database::engine(&directory.path().join("assets.turso"))
        .await
        .unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute(
            "CREATE TABLE agent_blobs(digest TEXT PRIMARY KEY,bytes BLOB NOT NULL)",
            (),
        )
        .await
        .unwrap();
    (database, connection)
}

#[tokio::test]
async fn preserves_json_and_revisions() {
    let directory = tempfile::tempdir().unwrap();
    let (_database, connection) = database(&directory).await;
    let original = json!({
        "raw":{"mime_type":"image/png","data":[0,1,2,253,254,255]},
        "content":[{"type":"image","mimeType":"image/png","data":"AAEC/f7/"}],
        "a/b~c":{"resource":{"mimeType":"application/octet-stream","blob":"AAEC/f7/"}},
        "text":"unchanged"
    });
    let revision = blake3::hash(original.to_string().as_bytes());
    let payload = capture(&connection, original.clone()).await.unwrap();
    assert_eq!(payload.assets.len(), 3);
    assert_eq!(payload.value["raw"]["data"], Value::Null);
    assert_eq!(payload.value["content"][0]["data"], Value::Null);
    let restored = hydrate(&connection, &payload).await.unwrap();
    assert_eq!(restored, original);
    assert_eq!(blake3::hash(restored.to_string().as_bytes()), revision);
    let count: i64 = connection
        .prepare("SELECT count(*) FROM agent_blobs")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn preserves_noncanonical_text() {
    let directory = tempfile::tempdir().unwrap();
    let (_database, connection) = database(&directory).await;
    for text in ["Zg", "not base64", "Zg==\n"] {
        let original = json!({"mimeType":"text/plain","data":text});
        let payload = capture(&connection, original.clone()).await.unwrap();
        assert!(payload.assets.is_empty());
        assert_eq!(hydrate(&connection, &payload).await.unwrap(), original);
    }
}
