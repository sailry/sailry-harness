//! Session-owned models and shared HTTP clients for captured configurations.
use super::*;
use sailry_protocol::SessionId;
use std::{collections::BTreeMap, sync::Mutex, time::Duration};
use tokio::time::Instant;

const IDLE: Duration = Duration::from_secs(15 * 60);
const CAPACITY: usize = 128;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const READ_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Default)]
pub(crate) struct Registry {
    entries: Mutex<BTreeMap<(SessionId, String), Entry>>,
    http: Mutex<BTreeMap<String, reqwest::Client>>,
    limits: Mutex<BTreeMap<(SessionId, String), Limit>>,
}

struct Entry {
    provider: Arc<dyn Model>,
    used: Instant,
}

struct Limit {
    value: Option<usize>,
    failed: bool,
    used: Instant,
}

impl Registry {
    pub(super) fn context(&self, session: SessionId, fingerprint: &str) -> Option<Option<usize>> {
        let mut limits = self.limits.lock().unwrap();
        limits.retain(|_, limit| {
            limit.used.elapsed()
                < if limit.failed {
                    Duration::from_secs(60)
                } else {
                    IDLE
                }
        });
        limits
            .get(&(session, fingerprint.to_owned()))
            .map(|limit| limit.value)
    }

    pub(super) fn remember(
        &self,
        session: SessionId,
        fingerprint: String,
        value: Option<usize>,
        failed: bool,
    ) {
        let mut limits = self.limits.lock().unwrap();
        if limits.len() >= CAPACITY
            && let Some(oldest) = limits
                .iter()
                .min_by_key(|(_, limit)| limit.used)
                .map(|(key, _)| key.clone())
        {
            limits.remove(&oldest);
        }
        limits.insert(
            (session, fingerprint),
            Limit {
                value,
                failed,
                used: Instant::now(),
            },
        );
    }
    pub(super) fn get(
        &self,
        session: SessionId,
        fingerprint: String,
        create: impl FnOnce() -> Result<Arc<dyn Model>, Fault>,
    ) -> Result<Arc<dyn Model>, Fault> {
        let now = Instant::now();
        let mut entries = self.entries.lock().unwrap();
        Self::prune(&mut entries, now);
        let key = (session, fingerprint);
        if let Some(entry) = entries.get_mut(&key) {
            entry.used = now;
            return Ok(entry.provider.clone());
        }
        let provider = create()?;
        if entries.len() >= CAPACITY
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
        {
            entries.remove(&oldest);
        }
        entries.insert(
            key,
            Entry {
                provider: provider.clone(),
                used: now,
            },
        );
        Ok(provider)
    }

    pub(super) fn http(&self, origin: Option<url::Origin>) -> Result<reqwest::Client, Fault> {
        let key = origin
            .as_ref()
            .map_or_else(String::new, url::Origin::ascii_serialization);
        let mut clients = self.http.lock().unwrap();
        if let Some(client) = clients.get(&key) {
            return Ok(client.clone());
        }
        let client = if let Some(origin) = origin {
            // Adapted from Goose ee8500d5 api_client.rs (Apache-2.0; see
            // third_party_licenses/goose-Apache-2.0.txt). ApiClient's
            // decorator must not replace its authenticated transport with a looser one.
            reqwest::Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .read_timeout(READ_TIMEOUT)
                .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                    if attempt.previous().len() >= 10 {
                        return attempt.error("too many redirects");
                    }
                    if attempt.url().origin() == origin {
                        attempt.follow()
                    } else {
                        attempt.error("redirect crosses the authenticated request origin")
                    }
                }))
                .build()
                .map_err(factory::unavailable)?
        } else {
            reqwest::Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .read_timeout(READ_TIMEOUT)
                .build()
                .map_err(factory::unavailable)?
        };
        clients.insert(key, client.clone());
        Ok(client)
    }

    pub(crate) fn evict(&self, session: SessionId) {
        self.limits
            .lock()
            .unwrap()
            .retain(|(owner, _), _| *owner != session);
        self.entries
            .lock()
            .unwrap()
            .retain(|(owner, _), _| *owner != session);
    }

    pub(crate) fn clear(&self) {
        self.limits.lock().unwrap().clear();
        self.entries.lock().unwrap().clear();
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

    fn prune(entries: &mut BTreeMap<(SessionId, String), Entry>, now: Instant) {
        entries.retain(|_, entry| now.duration_since(entry.used) < IDLE);
    }
}

pub(super) fn fingerprint(
    provider: &Provider,
    config: &SessionConfig,
    revision: Option<u64>,
) -> Result<String, Fault> {
    let bytes = serde_json::to_vec(&(provider, config, revision))
        .map_err(|_| Fault::invalid("captured model configuration is invalid"))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}
