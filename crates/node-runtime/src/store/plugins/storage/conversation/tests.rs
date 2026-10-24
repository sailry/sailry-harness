use super::*;
use sailry_protocol::{NodeId, plugin};
use serde_json::json;

async fn database() -> Connection {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../../schema.sql"))
        .await
        .unwrap();
    db
}

async fn session(db: &Connection) -> SessionId {
    let session = SessionId::new();
    let row = crate::store::database::counters::next(
        db,
        crate::store::database::counters::Namespace::Sessions,
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO sessions(rowid,id,revision) VALUES(?1,?2,1)",
        params![row as i64, session.to_string()],
    )
    .await
    .unwrap();
    session
}

#[tokio::test]
async fn bounds_values_and_namespace() {
    let mut db = database().await;
    let transaction = db.transaction().await.unwrap();
    let scope = session(&transaction).await;
    let value = json!("x".repeat(storage::MAX_VALUE_BYTES));
    assert_eq!(
        write(
            &transaction,
            "fixture",
            scope,
            None,
            "entry",
            Some(&value),
            0
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(sequence(&transaction).await.unwrap(), 0);
    for index in 0..storage::MAX_NAMESPACE_KEYS {
        let key = index.to_string();
        let checkpoint = record(&transaction, "fixture", &key, Some(b"null"))
            .await
            .unwrap();
        project(
            &transaction,
            "fixture",
            scope,
            &key,
            1,
            Some(checkpoint),
            false,
        )
        .await
        .unwrap();
    }
    transaction.commit().await.unwrap();
    let transaction = db.transaction().await.unwrap();
    assert_eq!(
        write(
            &transaction,
            "fixture",
            scope,
            None,
            "another",
            Some(&json!(true)),
            0
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::Busy
    );
    let removed = write(&transaction, "fixture", scope, None, "0", None, 1)
        .await
        .unwrap();
    assert!(!removed.present);
    let added = write(
        &transaction,
        "fixture",
        scope,
        None,
        "another",
        Some(&json!(null)),
        0,
    )
    .await
    .unwrap();
    assert!(added.present);
    let other = write(
        &transaction,
        "fixture",
        session(&transaction).await,
        None,
        "another",
        Some(&json!(true)),
        0,
    )
    .await
    .unwrap();
    assert!(other.present);
    transaction.commit().await.unwrap();
}

#[tokio::test]
async fn requires_conversation() {
    let db = database().await;
    let request = Request::new(
        NodeId([1; 32]),
        Command::ReadPluginConversationValue {
            key: "entry".into(),
        },
    )
    .with_plugin(plugin::Context {
        package: plugin::Reference {
            name: "fixture".into(),
            digest: "fixture".into(),
            settings_revision: 0,
        },
        worktree: None,
        session: None,
        turn: None,
        invocation: None,
        surface: Default::default(),
    });
    assert_eq!(
        execute(&db, &request, None).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
}
