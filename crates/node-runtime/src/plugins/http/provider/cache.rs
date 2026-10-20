//! Node-scoped, account-bound read results share one acquisition per request.
use sailry_link::CancellationToken;
use sailry_protocol::{Credential, ErrorCode, Fault, plugin::http};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

type Slot = Arc<tokio::sync::Mutex<Option<Document>>>;
const CAPACITY: usize = 32;

#[derive(Clone, Copy)]
pub(crate) struct Policy {
    fingerprint: [u8; 32],
    ttl: Duration,
    requested: Instant,
}

impl Policy {
    pub(super) fn new(fingerprint: [u8; 32], ttl_ms: u32) -> Self {
        Self {
            fingerprint,
            ttl: Duration::from_millis(u64::from(ttl_ms)),
            requested: Instant::now(),
        }
    }

    pub(in crate::plugins) fn key(&self, credential: &Credential) -> [u8; 32] {
        let mut hash = blake3::Hasher::new();
        hash.update(&self.fingerprint);
        hash.update(credential.id.to_string().as_bytes());
        hash.update(&credential.revision.to_le_bytes());
        *hash.finalize().as_bytes()
    }
}

#[derive(Default)]
pub(crate) struct Cache(Mutex<BTreeMap<[u8; 32], (Instant, Slot)>>);

struct Document {
    result: Result<http::Response, Fault>,
    fetched: Instant,
}

impl Document {
    fn reusable(&self, policy: Policy) -> bool {
        // Failures are shared only with requests that overlapped acquisition.
        // A later read can recover immediately, without replaying a mutation.
        self.fetched >= policy.requested
            || self.result.as_ref().is_ok_and(|response| {
                (200..300).contains(&response.status) && self.fetched.elapsed() < policy.ttl
            })
    }
}

impl Cache {
    fn slot(&self, key: [u8; 32]) -> Result<Slot, Fault> {
        let mut entries = self.0.lock().map_err(|_| {
            Fault::new(
                ErrorCode::Internal,
                "provider resource cache is unavailable",
            )
        })?;
        if let Some((used, slot)) = entries.get_mut(&key) {
            *used = Instant::now();
            return Ok(slot.clone());
        }
        if entries.len() >= CAPACITY {
            let oldest = entries
                .iter()
                .filter(|(_, (_, slot))| Arc::strong_count(slot) == 1)
                .min_by_key(|(_, (used, _))| *used)
                .map(|(key, _)| *key)
                .ok_or_else(|| {
                    Fault::new(
                        ErrorCode::Busy,
                        "provider resource cache capacity is exhausted",
                    )
                })?;
            entries.remove(&oldest);
        }
        let slot = Arc::new(tokio::sync::Mutex::new(None));
        entries.insert(key, (Instant::now(), slot.clone()));
        Ok(slot)
    }

    pub(in crate::plugins) async fn fetch<F, T>(
        &self,
        key: [u8; 32],
        policy: Policy,
        stop: &CancellationToken,
        load: F,
    ) -> Result<http::Response, Fault>
    where
        F: FnOnce() -> T,
        T: Future<Output = Result<http::Response, Fault>>,
    {
        let slot = self.slot(key)?;
        let mut current = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(interrupted()),
            current = slot.lock() => current,
        };
        if let Some(document) = current
            .as_ref()
            .filter(|document| document.reusable(policy))
        {
            return document.result.clone();
        }
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(interrupted()),
            result = load() => result,
        };
        *current = Some(Document {
            result: result.clone(),
            fetched: Instant::now(),
        });
        result
    }
}

fn interrupted() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "plugin HTTP request was interrupted",
    )
}

#[cfg(test)]
mod tests;
