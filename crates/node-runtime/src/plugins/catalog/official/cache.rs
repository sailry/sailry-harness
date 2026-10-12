//! One validated index per execution Node, shared across listings and update checks.
use super::*;
use std::time::{Duration, Instant};

// Match the existing public web cache's fifteen-minute freshness window.
const TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Default)]
pub(in crate::plugins) struct Cache(tokio::sync::Mutex<Option<Document>>);

struct Document {
    source: String,
    entries: Vec<Entry>,
    fetched: Instant,
}

impl Cache {
    pub(super) async fn fetch<F, Future>(
        &self,
        source: &str,
        force: bool,
        stop: &CancellationToken,
        load: F,
    ) -> Result<Vec<Entry>, Fault>
    where
        F: FnOnce() -> Future,
        Future: std::future::Future<Output = Result<Vec<Entry>, Fault>>,
    {
        let mut current = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(github::cancelled()),
            current = self.0.lock() => current,
        };
        if !force
            && let Some(document) = current.as_ref()
            && document.source == source
            && document.fetched.elapsed() < TTL
        {
            return Ok(document.entries.clone());
        }
        // Hold the slot during acquisition so ordinary simultaneous calls share one fetch.
        let entries = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(github::cancelled()),
            entries = load() => entries?,
        };
        if stop.is_cancelled() {
            return Err(github::cancelled());
        }
        *current = Some(Document {
            source: source.into(),
            entries: entries.clone(),
            fetched: Instant::now(),
        });
        Ok(entries)
    }
}

#[cfg(test)]
mod tests;
