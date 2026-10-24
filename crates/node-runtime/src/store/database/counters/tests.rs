use super::*;
use crate::store::database::{connect, engine, schema::backup};
use std::path::Path;

async fn open(path: &Path) -> (turso::Database, Connection) {
    let database = engine(path).await.unwrap();
    let connection = connect(&database).await.unwrap();
    (database, connection)
}

async fn setup(connection: &Connection) {
    connection.execute_batch("CREATE TABLE counters(namespace TEXT PRIMARY KEY,value INTEGER NOT NULL CHECK(value>0)); CREATE TABLE events(cursor INTEGER PRIMARY KEY,body BLOB NOT NULL)").await.unwrap();
}

#[tokio::test]
async fn requires_the_business_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let (_database, connection) = open(&directory.path().join("node.turso")).await;
    setup(&connection).await;
    assert!(next(&connection, Namespace::Events).await.is_err());
    assert_eq!(
        connection
            .prepare("SELECT count(*) FROM counters")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn rolls_back_with_the_business_row() {
    let directory = tempfile::tempdir().unwrap();
    let (_database, connection) = open(&directory.path().join("node.turso")).await;
    setup(&connection).await;
    connection.execute("BEGIN CONCURRENT", ()).await.unwrap();
    let sequence = next(&connection, Namespace::Events).await.unwrap();
    connection
        .execute("INSERT INTO events VALUES(?1,X'01')", [sequence as i64])
        .await
        .unwrap();
    connection.execute("ROLLBACK", ()).await.unwrap();
    connection.execute("BEGIN CONCURRENT", ()).await.unwrap();
    assert_eq!(
        next(&connection, Namespace::Events).await.unwrap(),
        sequence
    );
    connection.execute("COMMIT", ()).await.unwrap();
    assert_eq!(
        connection
            .prepare("SELECT count(*) FROM events")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn retains_deleted_watermarks_after_restore() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node.turso");
    {
        let (_database, connection) = open(&path).await;
        setup(&connection).await;
        connection.execute("BEGIN CONCURRENT", ()).await.unwrap();
        let sequence = next(&connection, Namespace::Events).await.unwrap();
        connection
            .execute("INSERT INTO events VALUES(?1,X'01')", [sequence as i64])
            .await
            .unwrap();
        connection.execute("COMMIT", ()).await.unwrap();
        connection.execute("DELETE FROM events", ()).await.unwrap();
        connection
            .prepare("PRAGMA wal_checkpoint(TRUNCATE)")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap();
    }
    let snapshot = backup::preview(&path).unwrap();
    for path in [path.as_path(), snapshot.path()] {
        let (_database, connection) = open(path).await;
        connection.execute("BEGIN CONCURRENT", ()).await.unwrap();
        assert_eq!(next(&connection, Namespace::Events).await.unwrap(), 2);
        connection.execute("COMMIT", ()).await.unwrap();
    }
}

#[tokio::test]
async fn refuses_overflow_without_changing_the_watermark() {
    let directory = tempfile::tempdir().unwrap();
    let (_database, connection) = open(&directory.path().join("node.turso")).await;
    setup(&connection).await;
    connection
        .execute(
            "INSERT INTO counters VALUES('events',9223372036854775807)",
            (),
        )
        .await
        .unwrap();
    connection.execute("BEGIN CONCURRENT", ()).await.unwrap();
    assert!(next(&connection, Namespace::Events).await.is_err());
    connection.execute("ROLLBACK", ()).await.unwrap();
    assert_eq!(
        connection
            .prepare("SELECT value FROM counters")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        i64::MAX
    );
}

#[tokio::test]
async fn conflicting_allocations_do_not_commit_duplicate_rows() {
    let directory = tempfile::tempdir().unwrap();
    let (database, first) = open(&directory.path().join("node.turso")).await;
    setup(&first).await;
    first
        .execute("INSERT INTO counters VALUES('events',1)", ())
        .await
        .unwrap();
    let second = connect(&database).await.unwrap();
    first.execute("BEGIN CONCURRENT", ()).await.unwrap();
    second.execute("BEGIN CONCURRENT", ()).await.unwrap();
    // Freeze the second snapshot before the first allocation commits.
    second
        .prepare("SELECT value FROM counters")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap();
    let sequence = next(&first, Namespace::Events).await.unwrap();
    first
        .execute("INSERT INTO events VALUES(?1,X'01')", [sequence as i64])
        .await
        .unwrap();
    first.execute("COMMIT", ()).await.unwrap();
    let result = next(&second, Namespace::Events).await;
    if result.is_ok() {
        assert!(second.execute("COMMIT", ()).await.is_err());
    }
    if !second.is_autocommit().unwrap() {
        second.execute("ROLLBACK", ()).await.unwrap();
    }
    assert_eq!(
        first
            .prepare("SELECT count(*) FROM events")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        1
    );
    second.execute("BEGIN CONCURRENT", ()).await.unwrap();
    assert_eq!(next(&second, Namespace::Events).await.unwrap(), 3);
    second.execute("COMMIT", ()).await.unwrap();
}
