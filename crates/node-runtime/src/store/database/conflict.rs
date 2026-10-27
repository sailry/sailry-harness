use sailry_protocol::{Fault, FaultDetails};
use turso::{
    Connection, Error,
    transaction::{Transaction, TransactionBehavior},
};

use super::storage_error;

#[cfg(test)]
mod tests;

pub(in crate::store) const RETRIES: usize = 8;

pub(in crate::store) async fn begin(
    connection: &Connection,
) -> Result<turso::transaction::Transaction<'_>, Fault> {
    begin_with(connection, false).await
}

pub(in crate::store) async fn begin_with(
    connection: &Connection,
    schema: bool,
) -> Result<turso::transaction::Transaction<'_>, Fault> {
    begin_as(
        connection,
        if schema {
            // Turso requires its exclusive schema transaction for first-corpus DDL.
            TransactionBehavior::Deferred
        } else {
            TransactionBehavior::Concurrent
        },
    )
    .await
}

async fn begin_as(
    connection: &Connection,
    behavior: TransactionBehavior,
) -> Result<Transaction<'_>, Fault> {
    let mut transaction = Transaction::new_unchecked(connection, behavior)
        .await
        .map_err(storage_error)?;
    transaction.set_drop_behavior(turso::transaction::DropBehavior::Ignore);
    Ok(transaction)
}

/// Acquire the native read snapshot before any callback or external effect runs.
pub(in crate::store) async fn begin_read(
    connection: &Connection,
) -> Result<Transaction<'_>, Fault> {
    read_with(connection, |connection| Box::pin(start_read(connection))).await
}

async fn start_read(connection: &Connection) -> Result<Transaction<'_>, Fault> {
    let transaction = begin_as(connection, TransactionBehavior::Deferred).await?;
    // Turso 0.8.3 defers MVCC admission until the first table read. A schema
    // read pins that snapshot inside the retry boundary without replaying work.
    let mut rows = transaction
        .query("SELECT rootpage FROM sqlite_schema LIMIT 1", ())
        .await
        .map_err(storage_error)?;
    while rows.next().await.map_err(storage_error)?.is_some() {}
    drop(rows);
    Ok(transaction)
}

async fn read_with<'a>(
    connection: &'a Connection,
    mut begin: impl FnMut(&'a Connection) -> sailry_link::Pending<'a, Result<Transaction<'a>, Fault>>
    + Send,
) -> Result<Transaction<'a>, Fault> {
    for attempt in 0..=RETRIES {
        let error = match begin(connection).await {
            Ok(transaction) => return Ok(transaction),
            Err(error) => error,
        };
        rollback(connection).await?;
        let result = Err::<Transaction<'_>, _>(error);
        if !again(&result, attempt).await {
            return result;
        }
    }
    unreachable!("the final attempt returns its result")
}

/// Settle a single read without replaying its callback, even on a commit conflict.
pub(in crate::store) async fn finish_read<T>(
    transaction: Transaction<'_>,
    result: Result<T, Fault>,
) -> Result<T, Fault> {
    match result {
        Ok(value) => {
            commit(transaction).await?;
            Ok(value)
        }
        Err(error) => {
            rollback(&transaction).await?;
            Err(error)
        }
    }
}

pub(in crate::store) fn is_conflict(error: &Fault) -> bool {
    matches!(error.details, Some(FaultDetails::StorageConflict))
}

/// Call only after confirming rollback of an operation with no external effects.
pub(in crate::store) async fn again<T>(result: &Result<T, Fault>, attempt: usize) -> bool {
    if attempt >= RETRIES || !result.as_ref().is_err_and(is_conflict) {
        return false;
    }
    tokio::time::sleep(std::time::Duration::from_millis(1 << attempt.min(6))).await;
    true
}

pub(in crate::store) fn native(error: &Error) -> bool {
    // The pinned SDK exposes this MVCC conflict through its generic error variant.
    matches!(error, Error::Busy(_) | Error::BusySnapshot(_))
        || matches!(error, Error::Error(message) if matches!(message.as_str(), "Write-write conflict" | "Database schema conflict"))
}

/// Confirm that cleanup ended the transaction; this alone does not authorize retry.
/// The owner must have dropped its transaction or disabled deferred cleanup.
pub(in crate::store) async fn rollback(connection: &Connection) -> Result<(), Fault> {
    if connection.is_autocommit().map_err(storage_error)? {
        return Ok(());
    }
    let result = connection.execute("ROLLBACK", ()).await;
    // A dropped SDK transaction may consume its deferred rollback on this access,
    // leaving the explicit ROLLBACK with no active transaction to end.
    if !connection.is_autocommit().map_err(storage_error)? {
        result.map_err(storage_error)?;
        return Err(storage_error("rollback left a transaction active"));
    }
    Ok(())
}

/// Finish this native transaction before releasing its write or publication guard.
pub(in crate::store) async fn commit(
    mut transaction: turso::transaction::Transaction<'_>,
) -> Result<(), Fault> {
    transaction.set_drop_behavior(turso::transaction::DropBehavior::Ignore);
    match transaction.execute("COMMIT", ()).await {
        Ok(_) if transaction.is_autocommit().map_err(storage_error)? => Ok(()),
        Ok(_) => Err(storage_error("commit left a transaction active")),
        Err(error) => {
            let error = storage_error(error);
            rollback(&transaction).await?;
            Err(error)
        }
    }
}
