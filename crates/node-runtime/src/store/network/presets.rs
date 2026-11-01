//! Public relay suggestions are fetched by the addressed Node on explicit refresh.
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    network::{Catalog, Listing},
};
use std::{collections::BTreeSet, sync::Mutex, time::Duration};

// Source data preserves Codux's relay_presets.json at
// c9632fabe312e95809bae7ed73b520622ab68233 in crates/codux-remote-transport.
const URL: &str = "https://sailry.dev/catalogs/network-presets.json";
const BYTE_LIMIT: usize = 128 * 1024;
const PRESET_LIMIT: usize = 32;

#[derive(Default)]
pub(crate) struct Directory {
    cached: Mutex<Cached>,
    refreshing: tokio::sync::Mutex<()>,
    #[cfg(any(test, feature = "test-support"))]
    fixture: std::sync::RwLock<Option<String>>,
}

#[derive(Default)]
struct Cached {
    catalog: Option<Catalog>,
    unavailable: bool,
    completed: u64,
}

impl Cached {
    fn listing(&self) -> Listing {
        Listing {
            presets: self
                .catalog
                .as_ref()
                .map_or_else(Vec::new, |catalog| catalog.presets.clone()),
            unavailable: self.unavailable,
        }
    }
}

impl Directory {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn fixture(&self, endpoint: &str) -> Result<(), Fault> {
        let url = url::Url::parse(endpoint).map_err(|_| invalid())?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        *self.fixture.write().map_err(|_| unavailable())? = Some(url.to_string());
        Ok(())
    }

    pub(crate) async fn read(
        &self,
        refresh: bool,
        stop: &CancellationToken,
    ) -> Result<Listing, Fault> {
        if !refresh {
            return Ok(self.cached.lock().map_err(|_| unavailable())?.listing());
        }
        let completed = self.cached.lock().map_err(|_| unavailable())?.completed;
        let _refresh = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(cancelled()),
            refresh = self.refreshing.lock() => refresh,
        };
        {
            let cached = self.cached.lock().map_err(|_| unavailable())?;
            // Overlapping requests share the completed refresh, while a later
            // explicit refresh still reads the source again.
            if cached.completed != completed {
                return Ok(cached.listing());
            }
        }
        #[cfg(any(test, feature = "test-support"))]
        let source = self
            .fixture
            .read()
            .map_err(|_| unavailable())?
            .clone()
            .unwrap_or_else(|| URL.into());
        #[cfg(not(any(test, feature = "test-support")))]
        let source = URL.to_owned();
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(cancelled()),
            result = download(&source) => result,
        };
        let mut cached = self.cached.lock().map_err(|_| unavailable())?;
        cached.unavailable = result.is_err();
        if let Ok(catalog) = result {
            cached.catalog = Some(catalog);
        }
        cached.completed = cached.completed.wrapping_add(1);
        Ok(cached.listing())
    }
}

async fn download(source: &str) -> Result<Catalog, Fault> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| unavailable())?;
    let mut response = client
        .get(source)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| unavailable())?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > BYTE_LIMIT as u64)
    {
        return Err(unavailable());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
        if chunk.len() > BYTE_LIMIT.saturating_sub(bytes.len()) {
            return Err(unavailable());
        }
        bytes.extend_from_slice(&chunk);
    }
    parse(&bytes)
}

fn parse(bytes: &[u8]) -> Result<Catalog, Fault> {
    if bytes.len() > BYTE_LIMIT {
        return Err(invalid());
    }
    let catalog: Catalog = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if catalog.version != 1 || catalog.presets.len() > PRESET_LIMIT {
        return Err(invalid());
    }
    let mut ids = BTreeSet::new();
    for preset in &catalog.presets {
        if preset.id.is_empty()
            || preset.id.len() > 64
            || !preset
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || !ids.insert(&preset.id)
            || !preset.label.valid()
        {
            return Err(invalid());
        }
        super::transport(&preset.selection).validate()?;
    }
    Ok(catalog)
}

fn invalid() -> Fault {
    Fault::invalid("invalid network preset catalog")
}

fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        "network preset catalog is unavailable",
    )
}

fn cancelled() -> Fault {
    Fault::new(
        ErrorCode::Cancelled,
        "network preset catalog read cancelled",
    )
}

#[cfg(test)]
mod tests;
