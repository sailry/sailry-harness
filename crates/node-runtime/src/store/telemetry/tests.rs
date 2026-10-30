use super::*;
use crate::store::database::Database;
use sailry_protocol::{Command, NodeId};
use serde_json::json;

#[tokio::test]
async fn preserves_current_defaults_and_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node.turso");
    let node = NodeId([62; 32]);
    let database = Database::open(&path, node, None).await.unwrap();
    let mut settings = read(&database.connection).await.unwrap();
    settings.revision = 7;
    settings.errors = false;
    settings.metrics = false;
    write(&database.connection, &settings).await.unwrap();
    assert!(!pending_defaults(&database.connection).await.unwrap());
    database.close().await.unwrap();
    let database = Database::open(&path, node, None).await.unwrap();
    assert_eq!(read(&database.connection).await.unwrap(), settings);
    assert!(!directory.path().join("backups").exists());
    database.close().await.unwrap();
}

#[tokio::test]
async fn rejects_invalid_values_without_replacing_data() {
    for value in [
        json!({"revision":3,"errors":"off"}),
        json!({"revision":3,"metrics":null}),
        json!({"revision":3,"installation_id":"00000000-0000-0000-0000-000000000000"}),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node.turso");
        let node = NodeId([63; 32]);
        let database = Database::open(&path, node, None).await.unwrap();
        let body = serde_json::to_vec(&value).unwrap();
        database
            .connection
            .execute("UPDATE telemetry_settings SET body=?1", [body.as_slice()])
            .await
            .unwrap();
        database.close().await.unwrap();
        assert!(Database::open(&path, node, None).await.is_err());
        let handle = crate::store::database::engine(&path).await.unwrap();
        let connection = crate::store::database::connect(&handle).await.unwrap();
        assert_eq!(super::body(&connection).await.unwrap().unwrap(), body);
        assert!(!directory.path().join("backups").exists());
    }
}

#[test]
fn denies_plugin_consent_changes() {
    let mut request = Request::new(NodeId([64; 32]), Command::ReadTelemetrySettings);
    request.plugin = Some(sailry_protocol::plugin::Context {
        invocation: None,
        surface: sailry_protocol::plugin::desktop::Surface::Settings,
        package: sailry_protocol::plugin::Reference {
            name: "fixture".into(),
            settings_revision: 0,
            digest: "a".repeat(64),
        },
        worktree: None,
        session: None,
        turn: None,
    });
    assert_eq!(
        controller(&request).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
}
