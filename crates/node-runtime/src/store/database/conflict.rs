use sailry_protocol::{Fault, FaultDetails};
use turso::{Connection, Error};

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
    let mut transaction = turso::transaction::Transaction::new_unchecked(
        connection,
        if schema {
            // Turso requires its exclusive schema transaction for first-corpus DDL.
            turso::transaction::TransactionBehavior::Deferred
        } else {
            turso::transaction::TransactionBehavior::Concurrent
        },
    )
    .await
    .map_err(storage_error)?;
    transaction.set_drop_behavior(turso::transaction::DropBehavior::Ignore);
    Ok(transaction)
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
