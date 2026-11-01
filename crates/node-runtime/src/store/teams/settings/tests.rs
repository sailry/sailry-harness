use super::*;
use crate::store::database::Database;
use sailry_protocol::NodeId;

async fn open() -> (tempfile::TempDir, Database) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap().join("node.turso");
    let database = Database::open(&path, NodeId([35; 32]), None).await.unwrap();
    (root, database)
}

#[tokio::test]
async fn saves_with_exact_revision() {
    let (_root, database) = open().await;
    let db = &database.connection;
    let current = read(db).await.unwrap();
    assert_eq!(current, Settings::default());
    let (output, event) = save(
        db,
        &Settings {
            revision: 0,
            child_concurrency: 3,
        },
    )
    .await
    .unwrap();
    let Output::TeamSettings(saved) = output else {
        panic!("team settings output expected");
    };
    assert_eq!((saved.revision, saved.child_concurrency), (1, 3));
    assert!(matches!(
        event,
        Some(Event::TeamSettingsChanged { revision: 1 })
    ));
    assert_eq!(read(db).await.unwrap(), saved);
    assert_eq!(
        save(db, &Settings::default()).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        save(
            db,
            &Settings {
                revision: 1,
                child_concurrency: 0,
            },
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(read(db).await.unwrap(), saved);
}

#[tokio::test]
async fn imports_once() {
    let (_root, database) = open().await;
    let db = &database.connection;
    import(db, 16).await.unwrap();
    let imported = read(db).await.unwrap();
    assert_eq!((imported.revision, imported.child_concurrency), (1, 16));
    import(db, 2).await.unwrap();
    assert_eq!(read(db).await.unwrap(), imported);
    assert!(import(db, 0).await.is_ok());
    assert_eq!(read(db).await.unwrap(), imported);
}

#[tokio::test]
async fn rejects_invalid_stored_budget() {
    let (_root, database) = open().await;
    let db = &database.connection;
    db.execute(
        "INSERT INTO team_settings(singleton,body) VALUES(1,?1)",
        [br#"{"revision":1,"child_concurrency":0}"#.to_vec()],
    )
    .await
    .unwrap();
    assert!(read(db).await.is_err());
}
