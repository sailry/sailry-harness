use sailry_protocol::{ErrorCode, Fault, NodeId, Output, RequestId, RequestOutcome};
use turso::{Connection, params};

use super::database::storage_error;

#[cfg(test)]
mod tests;

/// Persist an already captured file outcome without invoking the file operation again.
pub(super) async fn complete(
    database: &mut super::Database,
    caller: NodeId,
    id: RequestId,
    result: &sailry_link::Response,
) -> Result<(), Fault> {
    use super::database::conflict;
    let body = serde_json::to_string(result).map_err(storage_error)?;
    let write = database.write().await?;
    for attempt in 0..=conflict::RETRIES {
        let recorded = async {
            let transaction = conflict::begin(&database.connection).await?;
            let status: String = transaction
                .prepare("SELECT status FROM requests WHERE caller=?1 AND id=?2")
                .await.map_err(storage_error)?
                .query_row(params![&caller.0[..], id.to_string()])
                .await.map_err(storage_error)?
                .get(0).map_err(storage_error)?;
            if status != "admitted" {
                return Err(storage_error("durable admission missing"));
            }
            #[cfg(test)]
            tests::before_write().await?;
            let changed = transaction.execute(
                "UPDATE requests SET status='completed',result=?3 WHERE caller=?1 AND id=?2 AND status='admitted'",
                params![&caller.0[..], id.to_string(), body.as_str()],
            ).await.map_err(storage_error)?;
            if changed != 1 {
                return Err(storage_error("durable admission missing"));
            }
            conflict::commit(transaction).await
        }.await;
        if let Err(error) = conflict::rollback(&database.connection).await {
            database.connection.hold_write(write);
            return Err(error);
        }
        if !conflict::again(&recorded, attempt).await {
            return recorded;
        }
    }
    unreachable!("the final attempt returns its result")
}

pub(super) fn failed(node: NodeId, caller: NodeId, id: RequestId, error: &Fault) {
    let mut entry =
        crate::logging::Entry::new(crate::logging::Area::Command, crate::logging::Phase::Failed);
    entry.context.node = Some(node);
    entry.context.caller = Some(caller);
    entry.context.request = Some(id);
    entry.operation = Some("record_outcome");
    entry.stage = Some("receipt");
    entry.code = Some(error.code);
    entry.cause_code = Some(if super::database::conflict::is_conflict(error) {
        "storage_conflict"
    } else {
        "receipt_failed"
    });
    entry.emit();
}

pub(super) async fn inspect(
    db: &Connection,
    caller: NodeId,
    id: RequestId,
    digest: &str,
) -> Result<RequestOutcome, Fault> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Fault::invalid("invalid request digest"));
    }
    let previous: Option<(Vec<u8>, String, Option<String>)> = {
        let mut statement = db
            .prepare("SELECT body,status,result FROM requests WHERE caller=?1 AND id=?2")
            .await
            .map_err(storage_error)?;
        match statement
            .query_row(params![&caller.0[..], id.to_string()])
            .await
        {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let Some((body, status, result)) = previous else {
        return Ok(RequestOutcome::NotAdmitted);
    };
    // Content-bearing admissions retain replay identity, not another copy of their input.
    let matches = match body
        .strip_prefix(b"file-v1:")
        .or_else(|| body.strip_prefix(b"command-v1:"))
        .or_else(|| body.strip_prefix(b"credential-v1:"))
    {
        Some(stored) => stored == digest.as_bytes(),
        None => blake3::hash(&body).to_hex().as_str() == digest,
    };
    if !matches {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "request identifier was already used for different content",
        ));
    }
    resolve(&status, result)
}

pub(super) fn resolve(status: &str, result: Option<String>) -> Result<RequestOutcome, Fault> {
    match status {
        "admitted" => Ok(RequestOutcome::Admitted),
        "unknown" => Ok(RequestOutcome::Unknown),
        "completed" => Ok(RequestOutcome::Completed(Box::new(decode(
            &result.ok_or_else(|| storage_error("missing result"))?,
        )?))),
        _ => Err(storage_error("invalid admission status")),
    }
}

/// Decode the existing receipt without changing its status, body, or replay identity.
pub(in crate::store) fn decode(body: &str) -> Result<Result<Output, Fault>, Fault> {
    serde_json::from_str(body)
        .map_err(|_| Fault::new(ErrorCode::Internal, "stored request outcome is invalid"))
}
