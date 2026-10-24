//! Open only stopped native fixture files. Running Nodes use `Node::with_storage`.
use std::{path::Path, time::Duration};

pub async fn open(path: &Path, readonly: bool) -> (turso::Database, turso::Connection) {
    assert!(path.is_file(), "the stopped fixture database must exist");
    let database = turso::Builder::new_local(path.to_str().unwrap())
        .experimental_index_method(true)
        .read_only(readonly)
        .build()
        .await
        .unwrap();
    let connection = database.connect().unwrap();
    connection
        .pragma_update("synchronous", "FULL")
        .await
        .unwrap();
    connection.pragma_update("foreign_keys", 1).await.unwrap();
    connection.busy_timeout(Duration::ZERO).unwrap();
    assert_eq!(
        connection
            .prepare("PRAGMA journal_mode")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<String>(0)
            .unwrap(),
        "mvcc"
    );
    if readonly {
        connection.pragma_update("query_only", 1).await.unwrap();
    }
    (database, connection)
}

pub async fn close(database: turso::Database, connection: turso::Connection) {
    let readonly: i64 = connection
        .prepare("PRAGMA query_only")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    if readonly == 0 {
        let mut rows = connection
            .query("PRAGMA wal_checkpoint(TRUNCATE)", ())
            .await
            .unwrap();
        while rows.next().await.unwrap().is_some() {}
    }
    drop(connection);
    drop(database);
}

pub fn fault(error: impl std::fmt::Display) -> sailry_protocol::Fault {
    sailry_protocol::Fault::new(sailry_protocol::ErrorCode::Internal, error.to_string())
}
