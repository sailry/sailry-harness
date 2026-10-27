use super::*;
use crate::store::database::Database;
use sailry_protocol::{ErrorCode, NodeId};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test(start_paused = true)]
async fn retries_busy_begin() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let database = engine(&directory.path().join("node.turso")).await?;
    let connection = connect(&database).await?;
    connection.pragma_update("query_only", 1).await?;
    let mut attempts = 0;
    let transaction = read_with(&connection, |connection| {
        attempts += 1;
        if attempts == 1 {
            Box::pin(async { Err(storage_error(Error::Busy("checkpoint".into()))) })
        } else {
            Box::pin(begin_as(connection, TransactionBehavior::Deferred))
        }
    })
    .await?;
    let result = integer(&transaction, "SELECT 42")
        .await
        .map_err(storage_error);
    assert_eq!(finish_read(transaction, result).await?, 42);
    assert_eq!(attempts, 2);
    assert!(connection.is_autocommit()?);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn retries_busy_snapshot_admission() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let database = engine(&directory.path().join("node.turso")).await?;
    let connection = connect(&database).await?;
    let mut attempts = 0;
    let transaction = read_with(&connection, |connection| {
        attempts += 1;
        let first = attempts == 1;
        Box::pin(async move {
            if first {
                let transaction = begin_as(connection, TransactionBehavior::Deferred).await?;
                assert!(!transaction.is_autocommit().map_err(storage_error)?);
                return Err(storage_error(Error::Busy("read admission".into())));
            }
            start_read(connection).await
        })
    })
    .await?;
    assert_eq!(attempts, 2);
    finish_read(transaction, Ok(())).await?;
    assert!(connection.is_autocommit()?);
    Ok(())
}

#[tokio::test]
async fn pins_snapshot_before_callback() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let database = engine(&directory.path().join("node.turso")).await?;
    let reader = connect(&database).await?;
    let writer = connect(&database).await?;
    writer
        .execute("CREATE TABLE items(value INTEGER)", ())
        .await?;
    writer.execute("INSERT INTO items VALUES(1)", ()).await?;
    reader.pragma_update("query_only", 1).await?;
    let transaction = begin_read(&reader).await?;
    writer.execute("INSERT INTO items VALUES(2)", ()).await?;
    let count = integer(&transaction, "SELECT count(*) FROM items")
        .await
        .map_err(storage_error);
    assert_eq!(finish_read(transaction, count).await?, 1);
    assert_eq!(integer(&reader, "SELECT count(*) FROM items").await?, 2);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn bounds_busy_attempts() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let database = engine(&directory.path().join("node.turso")).await?;
    let connection = connect(&database).await?;
    let mut attempts = 0;
    let result = read_with(&connection, |_| {
        attempts += 1;
        Box::pin(async { Err(storage_error(Error::Busy("checkpoint".into()))) })
    })
    .await;
    let error = result.expect_err("busy begin must exhaust its budget");
    assert_eq!(error.code, ErrorCode::Busy);
    assert!(is_conflict(&error));
    assert_eq!(attempts, RETRIES + 1);
    assert!(connection.is_autocommit()?);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn preserves_nonconflict_errors() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let database = engine(&directory.path().join("node.turso")).await?;
    let connection = connect(&database).await?;
    for error in [
        Fault::new(ErrorCode::Busy, "read capacity exhausted"),
        crate::store::unavailable(),
        storage_error(Error::Corrupt("invalid page".into())),
    ] {
        let mut attempts = 0;
        let result = read_with(&connection, |_| {
            attempts += 1;
            let error = error.clone();
            Box::pin(async move { Err(error) })
        })
        .await;
        assert_eq!(result.expect_err("begin must fail"), error);
        assert_eq!(attempts, 1);
        assert!(connection.is_autocommit()?);
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn stops_after_unconfirmed_rollback() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let database = engine(&directory.path().join("node.turso")).await?;
    let connection = connect(&database).await?;
    connection
        .execute("CREATE TABLE items(value INTEGER NOT NULL)", ())
        .await?;
    let transaction = begin(&connection).await?;
    let mut rows = transaction
        .query("INSERT INTO items VALUES(1),(2) RETURNING value", ())
        .await?;
    assert!(rows.next().await?.is_some());
    drop(transaction);
    let mut attempts = 0;
    let result = read_with(&connection, |_| {
        attempts += 1;
        Box::pin(async { Err(storage_error(Error::Busy("checkpoint".into()))) })
    })
    .await;
    assert!(result.is_err());
    assert_eq!(attempts, 1, "failed cleanup cannot authorize another begin");
    assert!(!connection.is_autocommit()?);
    while rows.next().await?.is_some() {}
    drop(rows);
    rollback(&connection).await?;
    assert!(connection.is_autocommit()?);
    assert_eq!(integer(&connection, "SELECT count(*) FROM items").await?, 0);
    Ok(())
}

#[tokio::test]
async fn rolls_back_failed_reads() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let database = engine(&directory.path().join("node.turso")).await?;
    let connection = connect(&database).await?;
    let transaction = begin_read(&connection).await?;
    let error = storage_error(Error::BusySnapshot("read snapshot".into()));
    assert_eq!(
        finish_read::<()>(transaction, Err(error.clone())).await,
        Err(error)
    );
    assert!(connection.is_autocommit()?);
    let transaction = begin_read(&connection).await?;
    finish_read(transaction, Ok(())).await?;
    assert!(connection.is_autocommit()?);
    Ok(())
}

#[tokio::test]
async fn keeps_callbacks_single_use() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let database =
        Database::open(&directory.path().join("node.turso"), NodeId([5; 32]), None).await?;
    let storage = database.into_storage();
    let calls = Arc::new(AtomicUsize::new(0));
    for error in [
        storage_error(Error::Busy("callback conflict".into())),
        crate::store::unavailable(),
    ] {
        let called = calls.clone();
        let expected = error.clone();
        let result = storage
            .read(move |connection| {
                Box::pin(async move {
                    called.fetch_add(1, Ordering::Relaxed);
                    assert!(!connection.is_autocommit().map_err(storage_error)?);
                    Err::<(), _>(error)
                })
            })
            .await;
        assert_eq!(result, Err(expected));
    }
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    let called = calls.clone();
    struct Payload(i64);
    let payload = Payload(42);
    let value = storage
        .read(move |connection| {
            Box::pin(async move {
                called.fetch_add(1, Ordering::Relaxed);
                assert!(!connection.is_autocommit().map_err(storage_error)?);
                Ok(payload)
            })
        })
        .await?;
    assert_eq!(value.0, 42);
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    storage.close().await?;
    Ok(())
}
