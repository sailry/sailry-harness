//! Node-owned native clients; turn wrappers and request credentials stay outside this cache.
use super::*;
use crate::store::login::Authorization;
use sailry_protocol::{Secret, SessionId};
use std::{
    collections::BTreeMap,
    sync::Mutex,
    time::{Duration, Instant},
};

const IDLE: Duration = Duration::from_secs(15 * 60);
const CAPACITY: usize = 128;

#[derive(Default)]
pub(crate) struct Registry {
    entries: Mutex<BTreeMap<(SessionId, String), Cached>>,
}

struct Cached {
    entry: Arc<Entry>,
    used: Instant,
}

pub(super) struct Entry {
    pub inner: Arc<dyn Llm>,
    authorization: Mutex<Option<Authorization>>,
    secrets: Mutex<Vec<Secret>>,
}

impl Registry {
    pub(super) fn get(
        &self,
        session: SessionId,
        fingerprint: String,
        create: impl FnOnce() -> Result<Arc<dyn Llm>, Fault>,
    ) -> Result<Arc<Entry>, Fault> {
        let now = Instant::now();
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, cached| now.duration_since(cached.used) < IDLE);
        let key = (session, fingerprint);
        if let Some(cached) = entries.get_mut(&key) {
            cached.used = now;
            return Ok(cached.entry.clone());
        }
        let entry = Arc::new(Entry {
            inner: create()?,
            authorization: Mutex::default(),
            secrets: Mutex::default(),
        });
        // Reconfiguration replaces an idle entry, while in-flight handles keep their capture.
        entries
            .retain(|(owner, _), cached| *owner != session || Arc::strong_count(&cached.entry) > 1);
        if entries.len() >= CAPACITY
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, value)| value.used)
                .map(|(key, _)| key.clone())
        {
            entries.remove(&oldest);
        }
        entries.insert(
            key,
            Cached {
                entry: entry.clone(),
                used: now,
            },
        );
        Ok(entry)
    }

    pub(crate) fn evict(&self, session: SessionId) {
        self.entries
            .lock()
            .unwrap()
            .retain(|(owner, _), _| *owner != session);
    }

    pub(crate) fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }
}

impl Entry {
    pub(super) fn authorize(&self, authorization: Authorization) -> Authorization {
        let mut current = self.authorization.lock().unwrap();
        if current
            .as_ref()
            .is_none_or(|value| value.credential.revision <= authorization.credential.revision)
        {
            *current = Some(authorization);
        }
        current.as_ref().expect("authorization initialized").clone()
    }

    pub(super) fn protect(&self, current: Vec<Secret>) -> Vec<Secret> {
        let mut previous = self.secrets.lock().unwrap();
        let mut secrets = current.clone();
        for secret in previous.iter() {
            if !secrets
                .iter()
                .any(|value| value.expose() == secret.expose())
            {
                secrets.push(secret.clone());
            }
        }
        *previous = current;
        secrets
    }
}

pub(super) fn fingerprint(
    provider: &Provider,
    config: &SessionConfig,
    effort: Effort,
    revision: Option<u64>,
) -> Result<String, Fault> {
    let value = serde_json::to_vec(&(
        provider,
        &config.model,
        effort,
        &config.credential,
        revision,
    ))
    .map_err(|_| Fault::invalid("model client identity is invalid"))?;
    Ok(blake3::hash(&value).to_hex().to_string())
}

#[cfg(test)]
mod tests;
