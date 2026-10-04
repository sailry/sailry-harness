use super::*;
use sailry_protocol::{NodeId, plugin};
use serde_json::json;

fn database() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../../schema.sql"))
        .unwrap();
    db
}

fn session(db: &Connection) -> SessionId {
    let session = SessionId::new();
    db.execute(
        "INSERT INTO sessions(id,revision) VALUES(?1,1)",
        [session.to_string()],
    )
    .unwrap();
    session
}

#[test]
fn bounds_values_and_namespace() {
    let db = database();
    let scope = session(&db);
    let value = json!("x".repeat(storage::MAX_VALUE_BYTES));
    assert_eq!(
        write(&db, "fixture", scope, None, "entry", Some(&value), 0)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(sequence(&db).unwrap(), 0);
    let transaction = db.unchecked_transaction().unwrap();
    for index in 0..storage::MAX_NAMESPACE_KEYS {
        let key = index.to_string();
        let checkpoint = record(&transaction, "fixture", &key, Some(b"null")).unwrap();
        project(
            &transaction,
            "fixture",
            scope,
            &key,
            1,
            Some(checkpoint),
            false,
        )
        .unwrap();
    }
    transaction.commit().unwrap();
    assert_eq!(
        write(
            &db,
            "fixture",
            scope,
            None,
            "another",
            Some(&json!(true)),
            0
        )
        .unwrap_err()
        .code,
        ErrorCode::Busy
    );
    let removed = write(&db, "fixture", scope, None, "0", None, 1).unwrap();
    assert!(!removed.present);
    let added = write(
        &db,
        "fixture",
        scope,
        None,
        "another",
        Some(&json!(null)),
        0,
    )
    .unwrap();
    assert!(added.present);
    let other = write(
        &db,
        "fixture",
        session(&db),
        None,
        "another",
        Some(&json!(true)),
        0,
    )
    .unwrap();
    assert!(other.present);
}

#[test]
fn requires_conversation() {
    let db = database();
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
        execute(&db, &request, None).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
}
