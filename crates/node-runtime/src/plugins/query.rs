//! Bounded Node-owned read results; unrelated keys never wait for network work.
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const CAPACITY: usize = 32;
type Slot<V> = Arc<tokio::sync::Mutex<State<V>>>;
type Entries<K, V> = Vec<(K, Instant, Slot<V>)>;

pub(in crate::plugins) struct Cache<K, V>(Mutex<Entries<K, V>>);

impl<K, V> Default for Cache<K, V> {
    fn default() -> Self {
        Self(Mutex::new(Vec::new()))
    }
}

struct State<V> {
    good: Option<(V, Instant)>,
    attempt: Option<(Result<V, Fault>, Instant)>,
}
impl<V> Default for State<V> {
    fn default() -> Self {
        Self {
            good: None,
            attempt: None,
        }
    }
}

impl<K: Eq + Clone, V: Clone> Cache<K, V> {
    fn slot(&self, key: &K) -> Result<Slot<V>, Fault> {
        let mut entries = self
            .0
            .lock()
            .map_err(|_| Fault::new(ErrorCode::Internal, "read cache is unavailable"))?;
        if let Some((_, used, slot)) = entries.iter_mut().find(|(stored, _, _)| stored == key) {
            *used = Instant::now();
            return Ok(slot.clone());
        }
        if entries.len() == CAPACITY {
            let index = entries
                .iter()
                .enumerate()
                .filter(|(_, (_, _, slot))| Arc::strong_count(slot) == 1)
                .min_by_key(|(_, (_, used, _))| *used)
                .map(|(index, _)| index)
                .ok_or_else(|| Fault::new(ErrorCode::Busy, "read cache is busy"))?;
            entries.remove(index);
        }
        let slot = Arc::new(tokio::sync::Mutex::new(State::default()));
        entries.push((key.clone(), Instant::now(), slot.clone()));
        Ok(slot)
    }

    pub fn peek(&self, key: &K) -> Option<V> {
        let entries = self.0.lock().ok()?;
        let (_, _, slot) = entries.iter().find(|(stored, _, _)| stored == key)?;
        slot.try_lock()
            .ok()?
            .good
            .as_ref()
            .map(|(value, _)| value.clone())
    }

    pub fn insert(&self, key: K, value: V) -> Result<(), Fault> {
        let slot = self.slot(&key)?;
        let Ok(mut state) = slot.try_lock() else {
            return Ok(());
        };
        let completed = Instant::now();
        state.good = Some((value.clone(), completed));
        state.attempt = Some((Ok(value), completed));
        Ok(())
    }

    pub async fn fetch<F, T>(
        &self,
        key: K,
        ttl: Duration,
        force: bool,
        stop: &CancellationToken,
        load: F,
    ) -> Result<V, Fault>
    where
        F: FnOnce() -> T,
        T: Future<Output = Result<V, Fault>>,
    {
        let requested = Instant::now();
        let slot = self.slot(&key)?;
        let mut state = tokio::select! {biased;
            _ = stop.cancelled() => return Err(super::github::cancelled()),
            state = slot.lock() => state,
        };
        if let Some((result, completed)) = &state.attempt
            && *completed >= requested
        {
            return result.clone();
        }
        if !force
            && state
                .attempt
                .as_ref()
                .is_none_or(|(result, _)| result.is_ok())
            && let Some((value, fetched)) = &state.good
            && fetched.elapsed() < ttl
        {
            return Ok(value.clone());
        }
        let result = tokio::select! {biased;
            _ = stop.cancelled() => return Err(super::github::cancelled()),
            result = load() => result,
        };
        let completed = Instant::now();
        if let Ok(value) = &result {
            state.good = Some((value.clone(), completed));
        }
        state.attempt = Some((result.clone(), completed));
        result
    }
}

#[cfg(test)]
mod tests;
