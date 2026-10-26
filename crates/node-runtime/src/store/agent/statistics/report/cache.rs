//! Reuse canonical aggregates across read leases and transient conversation updates.
use super::*;
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::Mutex;

const ENTRIES: usize = 8;
const BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
pub(in crate::store) struct Cache {
    entries: Mutex<VecDeque<Cached>>,
    #[cfg(test)]
    scans: std::sync::atomic::AtomicUsize,
}

struct Cached {
    stamp: Stamp,
    report: Arc<Report>,
    bytes: usize,
}

#[derive(PartialEq, Eq)]
struct Stamp {
    usage: i64,
    responses: i64,
    metadata: i64,
    pricing: u64,
}

impl Cache {
    pub(in crate::store) async fn read(
        &self,
        db: &Connection,
        node: NodeId,
        query: &Query,
    ) -> Result<Report, Fault> {
        let (start, end) = query::validate(query)?;
        // This partial covering index excludes tools and text-only canonical events.
        let row = db
            .prepare(
                "SELECT coalesce(max(sequence),0),count(*) FROM agent_events
                 WHERE json_type(body,'$.usage_metadata')='object'
                   AND substr(json_extract(body,'$.timestamp'),1,19)>=?1
                   AND substr(json_extract(body,'$.timestamp'),1,19)<=?2",
            )
            .await
            .map_err(storage_error)?
            .query_row(params![
                start.format("%Y-%m-%dT%H:%M:%S").to_string(),
                end.format("%Y-%m-%dT%H:%M:%S").to_string(),
            ])
            .await
            .map_err(storage_error)?;
        let metadata = db
            .prepare(
                "SELECT coalesce(max(cursor),0) FROM events
                 WHERE json_extract(body,'$.kind') IN (
                   'project_registered','project_changed','project_removed',
                   'ssh_changed','ssh_removed','database_changed','database_removed',
                   'session_changed','sessions_removed','session_rewound')",
            )
            .await
            .map_err(storage_error)?
            .query_row(())
            .await
            .map_err(storage_error)?
            .get(0)
            .map_err(storage_error)?;
        let stamp = Stamp {
            usage: row.get(0).map_err(storage_error)?,
            responses: row.get(1).map_err(storage_error)?,
            metadata,
            pricing: crate::store::providers::catalog::status(db).await?.revision,
        };
        let mut entries = self.entries.lock().await;
        let mut report = if let Some(index) = entries.iter().position(|entry| {
            entry.stamp == stamp && entry.report.node == node && entry.report.query == *query
        }) {
            let cached = entries.remove(index).expect("matched cached report");
            let report = cached.report.as_ref().clone();
            entries.push_back(cached);
            report
        } else {
            #[cfg(test)]
            self.scans
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let report = super::read(db, node, query).await?;
            let bytes = serde_json::to_vec(&report).map_err(storage_error)?.len();
            entries.retain(|entry| entry.report.query != *query);
            while entries.len() >= ENTRIES
                || entries.iter().map(|entry| entry.bytes).sum::<usize>() + bytes > BYTES
            {
                if entries.pop_front().is_none() {
                    break;
                }
            }
            if bytes <= BYTES {
                entries.push_back(Cached {
                    stamp,
                    report: Arc::new(report.clone()),
                    bytes,
                });
            }
            report
        };
        drop(entries);
        // Refresh only the subscription watermark when unrelated events advance.
        report.cursor = db
            .prepare("SELECT coalesce(max(cursor),0) FROM events")
            .await
            .map_err(storage_error)?
            .query_row(())
            .await
            .map_err(storage_error)?
            .get::<i64>(0)
            .map_err(storage_error)? as u64;
        // Titles can be assigned by an unmetered response without changing usage.
        let mut titles = BTreeMap::new();
        for item in &mut report.requests.items {
            let title = if let Some(title) = titles.get(&item.session) {
                title
            } else {
                let title = crate::store::agent::activity::title(db, item.session).await?;
                titles
                    .entry(item.session)
                    .or_insert(Some(title).filter(|title| !title.trim().is_empty()))
            };
            item.session_title = title.clone();
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests;
