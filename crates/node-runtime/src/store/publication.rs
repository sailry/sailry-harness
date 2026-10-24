//! Commits publish their durable events and canonical feed changes in one order.
use sailry_protocol::{EventEnvelope, Fault, NodeId};
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard, broadcast};

#[derive(Clone, Default)]
pub(in crate::store) struct Publication(Arc<Mutex<()>>);

/// Acquire before stepping COMMIT, after the transaction's business SQL is done.
/// Retain through the canonical feed changes; partial output uses its own fast path.
pub(in crate::store) struct Guard {
    _order: OwnedMutexGuard<()>,
}

impl Publication {
    pub(in crate::store) async fn lock(self) -> Guard {
        Guard {
            _order: self.0.lock_owned().await,
        }
    }
}

impl Guard {
    pub(in crate::store) fn send(
        &self,
        events: &broadcast::Sender<EventEnvelope>,
        envelope: EventEnvelope,
    ) {
        let _ = events.send(envelope);
    }
}

/// Read a bounded page from the committed event log in the caller's read transaction.
pub(in crate::store) async fn read(
    connection: &turso::Connection,
    node: NodeId,
    after: u64,
    through: Option<u64>,
) -> Result<(u64, Vec<EventEnvelope>), Fault> {
    use super::database::storage_error;
    let through = match through {
        Some(cursor) => cursor,
        None => {
            let cursor = connection
                .prepare("SELECT coalesce(max(cursor),0) FROM events")
                .await
                .map_err(storage_error)?
                .query_row(())
                .await
                .map_err(storage_error)?
                .get::<i64>(0)
                .map_err(storage_error)?;
            u64::try_from(cursor).map_err(storage_error)?
        }
    };
    let mut rows = connection
        .query(
            "SELECT cursor,body FROM events WHERE cursor>?1 AND cursor<=?2 ORDER BY cursor LIMIT 128",
            turso::params![
                i64::try_from(after).map_err(storage_error)?,
                i64::try_from(through).map_err(storage_error)?
            ],
        )
        .await
        .map_err(storage_error)?;
    let mut events = Vec::new();
    let mut bytes = 0;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let body = row.get::<Vec<u8>>(1).map_err(storage_error)?;
        if !events.is_empty() && bytes + body.len() > 2 * 1024 * 1024 {
            break;
        }
        bytes += body.len();
        events.push(EventEnvelope {
            node,
            cursor: u64::try_from(row.get::<i64>(0).map_err(storage_error)?)
                .map_err(storage_error)?,
            event: serde_json::from_slice(&body).map_err(storage_error)?,
        });
    }
    Ok((through, events))
}
