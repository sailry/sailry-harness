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
    pub(super) capabilities: Arc<transport::Capabilities>,
}

pub(super) struct Built {
    pub inner: Arc<dyn Llm>,
    pub responses: Option<Arc<OpenAIResponsesClient>>,
}

impl Built {
    pub(super) fn new(inner: Arc<dyn Llm>) -> Self {
        Self {
            inner,
            responses: None,
        }
    }
}

struct Cached {
    entry: Arc<Entry>,
    used: Instant,
}

pub(super) struct Entry {
    pub inner: Arc<dyn Llm>,
    pub responses: Option<Arc<OpenAIResponsesClient>>,
    authorization: Mutex<Option<Authorization>>,
    secrets: Mutex<Vec<Secret>>,
    pending: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Registry {
    pub(super) fn get(
        &self,
        session: SessionId,
        fingerprint: String,
        create: impl FnOnce() -> Result<Built, Fault>,
    ) -> Result<Arc<Entry>, Fault> {
        let now = Instant::now();
        let mut entries = self.entries.lock().unwrap();
        Self::prune(&mut entries, now);
        let key = (session, fingerprint);
        if let Some(cached) = entries.get_mut(&key) {
            cached.used = now;
            return Ok(cached.entry.clone());
        }
        let built = create()?;
        let entry = Arc::new(Entry {
            inner: built.inner,
            responses: built.responses,
            authorization: Mutex::default(),
            secrets: Mutex::default(),
            pending: Mutex::default(),
        });
        if entries.len() >= CAPACITY
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, value)| value.used)
                .map(|(key, _)| key.clone())
            && let Some(cached) = entries.remove(&oldest)
        {
            cached.entry.close();
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
        self.entries.lock().unwrap().retain(|(owner, _), cached| {
            if *owner == session {
                cached.entry.close();
                false
            } else {
                true
            }
        });
    }

    pub(crate) fn clear(&self) {
        let mut entries = self.entries.lock().unwrap();
        for cached in entries.values() {
            cached.entry.close();
        }
        entries.clear();
    }

    pub(crate) fn abandon(&self, session: SessionId) {
        for ((owner, _), cached) in self.entries.lock().unwrap().iter() {
            if *owner == session {
                cached.entry.abandon();
            }
        }
    }

    pub(crate) async fn maintain(registry: std::sync::Weak<Self>, stop: CancellationToken) {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            tokio::select! {
                _ = stop.cancelled() => break,
                _ = interval.tick() => {
                    let Some(registry) = registry.upgrade() else { break; };
                    Self::prune(&mut registry.entries.lock().unwrap(), Instant::now());
                }
            }
        }
    }

    fn prune(entries: &mut BTreeMap<(SessionId, String), Cached>, now: Instant) {
        entries.retain(|_, cached| {
            if now.duration_since(cached.used) >= IDLE {
                cached.entry.close();
                false
            } else {
                if let Some(socket) = cached
                    .entry
                    .responses
                    .as_ref()
                    .and_then(|client| client.websocket())
                {
                    socket.expire();
                }
                true
            }
        });
    }
}

impl Entry {
    pub(super) fn preconnect(
        &self,
        stop: &CancellationToken,
        connect: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        let Some(socket) = self
            .responses
            .as_ref()
            .and_then(|client| client.websocket())
        else {
            return;
        };
        if socket.is_connected() || stop.is_cancelled() {
            return;
        }
        let mut pending = self.pending.lock().unwrap();
        if pending.as_ref().is_some_and(|task| !task.is_finished()) {
            return;
        }
        let stop = stop.clone();
        *pending = Some(tokio::spawn(async move {
            tokio::select! {
                biased;
                _ = stop.cancelled() => {},
                _ = async { connect.await; tokio::time::sleep(Duration::from_secs(25)).await; } => {},
            }
            socket.close_unused();
        }));
    }

    fn abandon(&self) {
        if let Some(pending) = self.pending.lock().unwrap().take() {
            pending.abort();
        }
        if let Some(socket) = self
            .responses
            .as_ref()
            .and_then(|client| client.websocket())
        {
            socket.close_unused();
        }
    }

    fn close(&self) {
        self.abandon();
        if let Some(socket) = self
            .responses
            .as_ref()
            .and_then(|client| client.websocket())
        {
            socket.close_idle();
        }
    }

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

impl Drop for Entry {
    fn drop(&mut self) {
        self.close();
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
