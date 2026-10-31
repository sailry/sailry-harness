//! Reuse a session's event-derived statistics while its canonical inputs are unchanged.
//!
//! Canonical events are append-only with a global sequence, so the newest selected
//! event plus rewind revision and indexed turn tips identify inputs without decoding
//! bodies. Commands that only change queue or run state reuse the aggregate.
use super::*;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

const ENTRIES: usize = 32;

#[derive(Default)]
pub(in crate::store) struct Cache {
    entries: std::sync::Mutex<VecDeque<Entry>>,
    #[cfg(test)]
    misses: std::sync::atomic::AtomicUsize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Stamp {
    latest: i64,
    latest_id: Option<String>,
    history_revision: u64,
    tips: BTreeMap<String, (i64, i64)>,
    delegations: i64,
    pricing: u64,
    media: Vec<(String, Vec<u8>)>,
}

pub(in crate::store) struct Entry {
    session: SessionId,
    stamp: Stamp,
    canonical: Arc<Canonical>,
}

impl Entry {
    pub(super) fn new(session: SessionId, stamp: Stamp, canonical: Arc<Canonical>) -> Self {
        Self {
            session,
            stamp,
            canonical,
        }
    }
}

impl Cache {
    pub(super) fn find(&self, session: SessionId, stamp: &Stamp) -> Option<Arc<Canonical>> {
        let mut entries = self.entries.lock().unwrap();
        let Some(index) = entries
            .iter()
            .position(|entry| entry.session == session && entry.stamp == *stamp)
        else {
            #[cfg(test)]
            self.misses
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return None;
        };
        let entry = entries.remove(index)?;
        let canonical = entry.canonical.clone();
        entries.push_front(entry);
        Some(canonical)
    }

    pub(super) fn previous(
        &self,
        session: SessionId,
        stamp: &Stamp,
    ) -> Option<(i64, Arc<Canonical>)> {
        let entries = self.entries.lock().unwrap();
        let entry = entries.iter().find(|entry| entry.session == session)?;
        let old = &entry.stamp;
        if old.history_revision != stamp.history_revision
            || old.pricing != stamp.pricing
            || old.media != stamp.media
            || old.delegations != stamp.delegations
            || old.latest >= stamp.latest
        {
            return None;
        }
        // Every retained turn remains append-only. Newly selected historical
        // turns must rebuild rather than skipping events below the watermark.
        if old.tips.iter().any(|(turn, (first, last))| {
            !stamp
                .tips
                .get(turn)
                .is_some_and(|(next_first, next_last)| next_first == first && next_last >= last)
        }) || stamp
            .tips
            .iter()
            .any(|(turn, (first, _))| *first <= old.latest && !old.tips.contains_key(turn))
        {
            return None;
        }
        Some((old.latest, entry.canonical.clone()))
    }

    /// Only committed snapshots may be retained.
    pub(in crate::store) fn retain(&self, entry: Entry) {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|current| current.session != entry.session);
        entries.push_front(entry);
        entries.truncate(ENTRIES);
    }

    #[cfg(test)]
    pub(super) fn misses(&self) -> usize {
        self.misses.load(std::sync::atomic::Ordering::Relaxed)
    }
}

// Selected turns match report::session: visible turns plus their delegated children.
const SELECTED: &str = "WITH selected AS (
    SELECT turn FROM conversation_turns WHERE session=?1
    UNION
    SELECT d.turn FROM agent_delegations d
    JOIN conversation_turns h ON h.turn=d.parent_turn WHERE h.session=?1)";

pub(super) async fn stamp(db: &Connection, session: SessionId) -> Result<Stamp, Fault> {
    // Indexed reads only: event bodies are never decoded here.
    let scalars = async |sql: &str| -> Result<(i64, i64), Fault> {
        let row = db
            .prepare(sql)
            .await
            .map_err(storage_error)?
            .query_row([session.to_string()])
            .await
            .map_err(storage_error)?;
        Ok((
            row.get(0).map_err(storage_error)?,
            row.get(1).map_err(storage_error)?,
        ))
    };
    let mut tips = BTreeMap::new();
    let mut query = db
        .prepare(&format!(
            "{SELECTED} SELECT s.turn,
         (SELECT min(e.sequence) FROM agent_events e WHERE e.turn=s.turn),
         (SELECT max(e.sequence) FROM agent_events e WHERE e.turn=s.turn)
         FROM selected s ORDER BY s.turn"
        ))
        .await
        .map_err(storage_error)?;
    let mut rows = query
        .query([session.to_string()])
        .await
        .map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        if let Some(first) = row.get::<Option<i64>>(1).map_err(storage_error)? {
            tips.insert(
                row.get::<String>(0).map_err(storage_error)?,
                (first, row.get::<i64>(2).map_err(storage_error)?),
            );
        }
    }
    drop(rows);
    drop(query);
    let latest = tips
        .iter()
        .map(|(_, (_, latest))| *latest)
        .max()
        .unwrap_or_default();
    let (delegations, _) = scalars(
        "SELECT count(*),0 FROM agent_delegations d
         JOIN conversation_turns h ON h.turn=d.parent_turn WHERE h.session=?1",
    )
    .await?;
    // A rolled-back sequence can be reused only by a different event identity.
    let latest_id = {
        let mut statement = db
            .prepare("SELECT id FROM agent_events WHERE sequence=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row(params![latest]).await {
            Ok(row) => Some(row.get::<String>(0).map_err(storage_error)?),
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let mut media = Vec::new();
    {
        let mut statement = db
            .prepare(&format!(
                "{SELECTED} SELECT DISTINCT m.session,m.body FROM selected s
                 JOIN turns t ON t.id=s.turn JOIN session_media m ON m.session=t.session
                 ORDER BY m.session"
            ))
            .await
            .map_err(storage_error)?;
        let mut rows = statement
            .query([session.to_string()])
            .await
            .map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            media.push((
                row.get::<String>(0).map_err(storage_error)?,
                row.get::<Vec<u8>>(1).map_err(storage_error)?,
            ));
        }
    }
    Ok(Stamp {
        latest,
        latest_id,
        history_revision: crate::store::sessions::history_revision(db, session).await?,
        tips,
        delegations,
        pricing: crate::store::providers::catalog::status(db).await?.revision,
        media,
    })
}
