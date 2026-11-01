//! Finite HTTPS relay probes leave the live Link endpoint and trust unchanged.
use futures::{StreamExt, stream};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    network::{Latency, Selection},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};

const CONCURRENCY: usize = 4;
const CACHE_LIMIT: usize = 32;
const CACHE_TTL: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

type Slot = Arc<AsyncMutex<Option<Sample>>>;

pub(crate) struct Probes {
    cache: Mutex<VecDeque<(Vec<String>, Slot)>>,
    permits: Semaphore,
    #[cfg(any(test, feature = "test-support"))]
    roots: std::sync::RwLock<Option<sailry_link::CaTlsConfig>>,
}

impl Default for Probes {
    fn default() -> Self {
        Self {
            cache: Mutex::default(),
            permits: Semaphore::new(CONCURRENCY),
            #[cfg(any(test, feature = "test-support"))]
            roots: std::sync::RwLock::default(),
        }
    }
}

struct Sample {
    measured: Instant,
    millis: Option<u64>,
}

impl Probes {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn roots(&self, roots: sailry_link::CaTlsConfig) {
        *self.roots.write().unwrap() = Some(roots);
        self.cache.lock().unwrap().clear();
    }

    fn slot(&self, urls: &[String]) -> Result<Slot, Fault> {
        let mut cache = self.cache.lock().map_err(|_| unavailable())?;
        let slot = if let Some(index) = cache.iter().position(|(key, _)| key == urls) {
            cache.remove(index).unwrap().1
        } else {
            if cache.len() >= CACHE_LIMIT {
                cache.pop_front();
            }
            Arc::default()
        };
        cache.push_back((urls.to_vec(), slot.clone()));
        Ok(slot)
    }

    pub(crate) async fn read(
        &self,
        selection: &Selection,
        stop: &CancellationToken,
    ) -> Result<Latency, Fault> {
        let mut urls = super::transport(selection).urls()?;
        urls.sort();
        urls.dedup();
        let slot = self.slot(&urls)?;
        let mut sample = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(cancelled()),
            sample = slot.lock() => sample,
        };
        if let Some(sample) = sample
            .as_ref()
            .filter(|sample| sample.measured.elapsed() < CACHE_TTL)
        {
            return Ok(Latency {
                selection: selection.clone(),
                millis: sample.millis,
            });
        }
        let client = self.client()?;
        let mut pending = stream::iter(urls)
            .map(|url| {
                let client = client.clone();
                async move {
                    let _permit = self.permits.acquire().await.ok()?;
                    probe(&client, &url).await
                }
            })
            .buffer_unordered(CONCURRENCY);
        let expires = tokio::time::sleep(PROBE_TIMEOUT);
        tokio::pin!(expires);
        let mut fastest: Option<Duration> = None;
        loop {
            tokio::select! {
                biased;
                _ = stop.cancelled() => return Err(cancelled()),
                _ = &mut expires => break,
                next = pending.next() => match next {
                    Some(Some(duration)) => fastest = Some(fastest.map_or(duration, |previous| previous.min(duration))),
                    Some(None) => {},
                    None => break,
                },
            }
        }
        let millis =
            fastest.map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
        *sample = Some(Sample {
            measured: Instant::now(),
            millis,
        });
        Ok(Latency {
            selection: selection.clone(),
            millis,
        })
    }

    fn client(&self) -> Result<reqwest::Client, Fault> {
        let builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(REQUEST_TIMEOUT)
            .pool_max_idle_per_host(0);
        #[cfg(any(test, feature = "test-support"))]
        let builder = match self.roots.read().map_err(|_| unavailable())?.as_ref() {
            Some(roots) => builder.tls_backend_preconfigured(
                roots
                    .client_config(Arc::new(rustls::crypto::ring::default_provider()))
                    .map_err(|_| unavailable())?,
            ),
            None => builder,
        };
        builder.build().map_err(|_| unavailable())
    }
}

async fn probe(client: &reqwest::Client, address: &str) -> Option<Duration> {
    let base = reqwest::Url::parse(address).ok()?;
    // iroh 1.3.0 net_report uses the same /ping HTTPS response measurement.
    // This includes connection establishment and does not attest relay authentication.
    let url = base.join("/ping").ok()?;
    let started = Instant::now();
    let mut response = client.get(url).send().await.ok()?;
    let elapsed = started.elapsed();
    if !response.status().is_success() {
        return None;
    }
    let mut remaining: usize = 8 * 1024;
    while let Ok(Some(chunk)) = response.chunk().await {
        remaining = remaining.saturating_sub(chunk.len());
        if remaining == 0 {
            break;
        }
    }
    Some(elapsed)
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "network probe is unavailable")
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "network probe cancelled")
}

#[cfg(test)]
mod tests;
