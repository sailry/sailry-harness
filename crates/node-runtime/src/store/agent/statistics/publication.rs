//! Coalesce derived reads under the Node's existing task supervisor.
use super::*;
use crate::store::database::{Handle, conflict};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

#[derive(Default)]
pub(super) struct State {
    tasks: OnceLock<crate::tasks::Tasks>,
    pending: Mutex<BTreeMap<SessionId, bool>>,
    #[cfg(any(test, feature = "test-support"))]
    gates: Mutex<BTreeMap<SessionId, fixture::Gate>>,
    #[cfg(test)]
    reads: std::sync::atomic::AtomicUsize,
}

impl Cache {
    pub(in crate::store) fn configure(&self, tasks: crate::tasks::Tasks) {
        assert!(
            self.publication.tasks.set(tasks).is_ok(),
            "statistics owner initializes once"
        );
    }
}

struct Flight {
    cache: Arc<Cache>,
    feeds: Feeds,
    node: NodeId,
    session: SessionId,
    finished: bool,
}

impl Drop for Flight {
    fn drop(&mut self) {
        if !self.finished {
            let mut pending = self.cache.publication.pending.lock().unwrap();
            pending.remove(&self.session);
            self.feeds.publish(
                self.node,
                self.session,
                Change::StatisticsFailed(Fault::new(
                    ErrorCode::Unavailable,
                    "statistics refresh did not complete",
                )),
            );
        }
    }
}

/// Call after this commit's canonical frames, while its publication guard is held.
pub(in crate::store) async fn publish(database: &Database, session: SessionId) {
    let cache = database.statistics.clone();
    if cache.publication.tasks.get().is_none() {
        // Standalone storage unit fixtures have no Node supervisor.
        #[cfg(test)]
        match read(&database.handle, &cache, session).await {
            Ok((statistics, entry)) => {
                cache.retain(entry);
                database.feeds.publish(
                    database.node,
                    session,
                    Change::Statistics(Box::new(statistics)),
                );
            }
            Err(error) => {
                database
                    .feeds
                    .publish(database.node, session, Change::StatisticsFailed(error))
            }
        }
        #[cfg(not(test))]
        unavailable(database, session);
        return;
    }
    {
        let mut pending = cache.publication.pending.lock().unwrap();
        if let Some(dirty) = pending.get_mut(&session) {
            *dirty = true;
            return;
        }
    }
    request(database, session);
}

/// Ensure a supervised refresh after reserving a committed conversation snapshot.
/// Call under the publication guard; an existing flight publishes after this reservation.
pub(in crate::store) fn request(database: &Database, session: SessionId) {
    let cache = database.statistics.clone();
    let Some(tasks) = cache.publication.tasks.get() else {
        unavailable(database, session);
        return;
    };
    {
        let mut pending = cache.publication.pending.lock().unwrap();
        if pending.contains_key(&session) {
            return;
        }
        pending.insert(session, false);
    }
    let flight = Flight {
        cache: cache.clone(),
        feeds: database.feeds.clone(),
        node: database.node,
        session,
        finished: false,
    };
    let handle = database.handle.clone();
    let publication = database.publication.clone();
    let node = database.node;
    // The supervisor owns completion even when the command caller disconnects.
    let _ = tasks.spawn(async move {
        let mut flight = flight;
        loop {
            #[cfg(test)]
            flight
                .cache
                .publication
                .reads
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            #[cfg(any(test, feature = "test-support"))]
            fixture::wait(&flight.cache, session, true).await;
            let result = read(&handle, &flight.cache, session).await;
            #[cfg(any(test, feature = "test-support"))]
            fixture::wait(&flight.cache, session, false).await;
            let _publication = publication.clone().lock().await;
            if finish(&mut flight, node, result) {
                return;
            }
        }
    });
}

fn finish(
    flight: &mut Flight,
    node: NodeId,
    result: Result<(Statistics, cache::Entry), Fault>,
) -> bool {
    let session = flight.session;
    let mut pending = flight.cache.publication.pending.lock().unwrap();
    let dirty = pending
        .get_mut(&session)
        .expect("statistics flight is registered");
    if *dirty {
        *dirty = false;
        // This snapshot is committed and useful for incremental reuse, but
        // cannot replace a newer batch's state in the controller feed.
        if let Ok((_, entry)) = result {
            flight.cache.retain(entry);
        }
        return false;
    }
    match result {
        Ok((statistics, entry)) => {
            flight.cache.retain(entry);
            flight
                .feeds
                .publish(node, session, Change::Statistics(Box::new(statistics)));
        }
        Err(error) => flight
            .feeds
            .publish(node, session, Change::StatisticsFailed(error)),
    }
    pending.remove(&session);
    flight.finished = true;
    true
}

fn unavailable(database: &Database, session: SessionId) {
    database.feeds.publish(
        database.node,
        session,
        Change::StatisticsFailed(Fault::new(
            ErrorCode::Unavailable,
            "statistics supervisor is unavailable",
        )),
    );
}

async fn read(
    handle: &Handle,
    cache: &Cache,
    session: SessionId,
) -> Result<(Statistics, cache::Entry), Fault> {
    let connection = handle.lease(true).await.map_err(storage_error)?;
    let transaction = conflict::begin_read(&connection).await?;
    let result = super::prepare(cache, &transaction, session).await;
    conflict::finish_read(transaction, result).await
}

#[cfg(any(test, feature = "test-support"))]
mod fixture;

#[cfg(test)]
mod tests;
