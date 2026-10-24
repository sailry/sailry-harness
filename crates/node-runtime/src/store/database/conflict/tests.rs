use turso::transaction::{DropBehavior, Transaction, TransactionBehavior};

use super::*;
use crate::store::database::{connect, engine};

async fn integer(connection: &Connection, sql: &str) -> turso::Result<i64> {
    connection.prepare(sql).await?.query_row(()).await?.get(0)
}

#[test]
fn recognizes_native_conflicts() {
    for error in [
        Error::Busy("opaque busy detail".into()),
        Error::BusySnapshot("opaque snapshot detail".into()),
        Error::Error("Write-write conflict".into()),
        Error::Error("Database schema conflict".into()),
    ] {
        assert!(native(&error), "{error:?}");
    }
}

#[test]
fn rejects_other_errors() {
    for error in [
        Error::ToSqlConversionFailure(Box::new(std::io::Error::other("Write-write conflict"))),
        Error::QueryReturnedNoRows,
        Error::ConversionFailure("Write-write conflict".into()),
        Error::Interrupt("Write-write conflict".into()),
        Error::Error("database is locked".into()),
        Error::Error("database snapshot is stale".into()),
        Error::Error("write-write conflict".into()),
        Error::Error("Write-write conflict ".into()),
        Error::Error("operation failed: Write-write conflict".into()),
        Error::Misuse("Write-write conflict".into()),
        Error::Constraint("Write-write conflict".into()),
        Error::Readonly("Write-write conflict".into()),
        Error::DatabaseFull("Write-write conflict".into()),
        Error::NotAdb("Write-write conflict".into()),
        Error::Corrupt("Write-write conflict".into()),
        Error::IoError(std::io::ErrorKind::Other, "Write-write conflict"),
        Error::BatchStatementFailed {
            index: 0,
            error: Box::new(Error::Busy("database is locked".into())),
            results: Vec::new(),
        },
        Error::BatchRollbackFailed {
            error: Box::new(Error::BusySnapshot("snapshot is stale".into())),
            rollback_error: Box::new(Error::Busy("database is locked".into())),
        },
    ] {
        assert!(!native(&error), "{error:?}");
    }
}

#[tokio::test]
async fn ends_active_transactions() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let database = engine(&directory.path().join("node.turso")).await?;
    let connection = connect(&database).await?;
    connection
        .execute("CREATE TABLE items(value INTEGER NOT NULL)", ())
        .await?;
    rollback(&connection).await?;
    let mut transaction =
        Transaction::new_unchecked(&connection, TransactionBehavior::Concurrent).await?;
    transaction.set_drop_behavior(DropBehavior::Ignore);
    transaction
        .execute("INSERT INTO items VALUES(1)", ())
        .await?;
    drop(transaction);
    assert!(!connection.is_autocommit()?);
    rollback(&connection).await?;
    assert!(connection.is_autocommit()?);
    assert_eq!(integer(&connection, "SELECT count(*) FROM items").await?, 0);

    let transaction =
        Transaction::new_unchecked(&connection, TransactionBehavior::Concurrent).await?;
    transaction
        .execute("INSERT INTO items VALUES(2)", ())
        .await?;
    transaction.commit().await?;
    rollback(&connection).await?;
    assert!(connection.is_autocommit()?);
    assert_eq!(integer(&connection, "SELECT value FROM items").await?, 2);
    let transaction =
        Transaction::new_unchecked(&connection, TransactionBehavior::Concurrent).await?;
    transaction
        .execute("INSERT INTO items VALUES(3)", ())
        .await?;
    drop(transaction);
    rollback(&connection).await?;
    assert!(connection.is_autocommit()?);
    assert_eq!(integer(&connection, "SELECT value FROM items").await?, 2);
    Ok(())
}

#[tokio::test]
async fn rejects_unfinished_writes() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let database = engine(&directory.path().join("node.turso")).await?;
    let connection = connect(&database).await?;
    connection
        .execute("CREATE TABLE items(value INTEGER NOT NULL)", ())
        .await?;
    let mut transaction =
        Transaction::new_unchecked(&connection, TransactionBehavior::Concurrent).await?;
    transaction.set_drop_behavior(DropBehavior::Ignore);
    let mut rows = transaction
        .query("INSERT INTO items VALUES(1),(2) RETURNING value", ())
        .await?;
    assert!(rows.next().await?.is_some());
    drop(transaction);
    assert!(!connection.is_autocommit()?);
    assert!(rollback(&connection).await.is_err());
    assert!(!connection.is_autocommit()?);
    while rows.next().await?.is_some() {}
    drop(rows);
    rollback(&connection).await?;
    assert!(connection.is_autocommit()?);
    assert_eq!(integer(&connection, "SELECT count(*) FROM items").await?, 0);
    Ok(())
}

#[tokio::test]
async fn clears_conflicted_transactions() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let database = engine(&directory.path().join("node.turso")).await?;
    let first = connect(&database).await?;
    let second = connect(&database).await?;
    first
        .execute_batch(
            "CREATE TABLE items(id INTEGER PRIMARY KEY,value INTEGER NOT NULL); \
             INSERT INTO items VALUES(1,10),(2,20)",
        )
        .await?;
    let first_transaction =
        Transaction::new_unchecked(&first, TransactionBehavior::Concurrent).await?;
    let mut second_transaction =
        Transaction::new_unchecked(&second, TransactionBehavior::Concurrent).await?;
    second_transaction.set_drop_behavior(DropBehavior::Ignore);
    assert_eq!(
        integer(&first_transaction, "SELECT value FROM items WHERE id=1").await?,
        10
    );
    assert_eq!(
        integer(&second_transaction, "SELECT value FROM items WHERE id=1").await?,
        10
    );
    second_transaction
        .execute("UPDATE items SET value=99 WHERE id=2", ())
        .await?;
    first_transaction
        .execute("UPDATE items SET value=30 WHERE id=1", ())
        .await?;
    first_transaction.commit().await?;
    let result = match second_transaction
        .execute("UPDATE items SET value=40 WHERE id=1", ())
        .await
    {
        Ok(_) => second_transaction.commit().await,
        Err(error) => {
            drop(second_transaction);
            Err(error)
        }
    };
    let error = result.unwrap_err();
    assert!(matches!(error, Error::Error(ref message) if message == "Write-write conflict"));
    assert!(native(&error), "{error:?}");
    rollback(&second).await?;
    assert!(second.is_autocommit()?);
    assert_eq!(
        integer(&second, "SELECT value FROM items WHERE id=1").await?,
        30
    );
    assert_eq!(
        integer(&second, "SELECT value FROM items WHERE id=2").await?,
        20
    );

    let fresh = connect(&database).await?;
    let transaction = Transaction::new_unchecked(&fresh, TransactionBehavior::Concurrent).await?;
    transaction
        .execute("UPDATE items SET value=40 WHERE id=1", ())
        .await?;
    transaction.commit().await?;
    assert_eq!(
        integer(&second, "SELECT value FROM items WHERE id=1").await?,
        40
    );
    assert_eq!(
        integer(&second, "SELECT value FROM items WHERE id=2").await?,
        20
    );
    Ok(())
}
