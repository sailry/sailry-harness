//! Public suggestions use one cancellable cache per Node, without configuration storage.
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    conversation::partners::{Catalog, Listing},
};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const URL: &str = "https://raw.githubusercontent.com/sailry/sailry-harness/main/assets/catalogs/model-partners.json";
const BUNDLED: &[u8] = include_bytes!("../../../../assets/catalogs/model-partners.json");
const BYTE_LIMIT: usize = 1024 * 1024;
const TTL: Duration = Duration::from_secs(15 * 60);
const FAILURE_TTL: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct Directory {
    document: Mutex<Option<Document>>,
    #[cfg(any(test, feature = "test-support"))]
    fixture: std::sync::RwLock<Option<String>>,
}

struct Document {
    source: String,
    listing: Listing,
    attempted: Instant,
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

    pub(crate) async fn read(&self, stop: &CancellationToken) -> Result<Listing, Fault> {
        #[cfg(any(test, feature = "test-support"))]
        let source = self
            .fixture
            .read()
            .map_err(|_| unavailable())?
            .clone()
            .unwrap_or_else(|| URL.into());
        #[cfg(not(any(test, feature = "test-support")))]
        let source = URL.to_owned();
        self.cached(&source, stop, download(&source)).await
    }

    async fn cached(
        &self,
        source: &str,
        stop: &CancellationToken,
        load: impl std::future::Future<Output = Result<Catalog, Fault>>,
    ) -> Result<Listing, Fault> {
        let mut document = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(cancelled()),
            document = self.document.lock() => document,
        };
        if let Some(current) = document.as_ref().filter(|current| current.source == source) {
            let ttl = if current.listing.unavailable {
                FAILURE_TTL
            } else {
                TTL
            };
            if current.attempted.elapsed() < ttl {
                return Ok(current.listing.clone());
            }
        }
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(cancelled()),
            result = load => result,
        };
        let listing = match result {
            Ok(catalog) => Listing {
                catalog,
                unavailable: false,
            },
            Err(_) => Listing {
                catalog: document
                    .as_ref()
                    .filter(|current| current.source == source)
                    .map(|current| current.listing.catalog.clone())
                    .unwrap_or_else(|| parse(BUNDLED).expect("bundled partner catalog is valid")),
                unavailable: true,
            },
        };
        *document = Some(Document {
            source: source.to_owned(),
            listing: listing.clone(),
            attempted: Instant::now(),
        });
        Ok(listing)
    }
}

async fn download(source: &str) -> Result<Catalog, Fault> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
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
            .is_some_and(|length| length > BYTE_LIMIT as u64)
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
    if catalog.version != 1 {
        return Err(invalid());
    }
    let mut ids = BTreeSet::new();
    for partner in &catalog.partners {
        if partner.id.trim().is_empty()
            || partner.name.trim().is_empty()
            || !ids.insert(&partner.id)
            || partner.routes.is_empty()
        {
            return Err(invalid());
        }
        for value in [&partner.description, &partner.badge].into_iter().flatten() {
            if value.trim().is_empty() {
                return Err(invalid());
            }
        }
        for value in [&partner.homepage, &partner.key_url].into_iter().flatten() {
            public_url(value)?;
        }
        if let Some(logo) = &partner.logo {
            // The UI resolves existing public provider assets; missing assets use its generic icon.
            if let Some(name) = logo
                .strip_prefix("icons/providers/")
                .and_then(|path| path.strip_suffix(".svg"))
            {
                if name.is_empty()
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                {
                    return Err(invalid());
                }
            } else {
                public_url(logo)?;
            }
        }
        let mut protocols = BTreeSet::new();
        for route in &partner.routes {
            if !protocols.insert(route.protocol) {
                return Err(invalid());
            }
            let url = super::endpoint_url(&route.endpoint).map_err(|_| invalid())?;
            if url.scheme() != "https" {
                return Err(invalid());
            }
        }
        let mut models = BTreeSet::new();
        for model in &partner.models {
            if model.trim().is_empty() || !models.insert(model) {
                return Err(invalid());
            }
        }
    }
    Ok(catalog)
}

fn public_url(value: &str) -> Result<(), Fault> {
    let url = url::Url::parse(value).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> Fault {
    Fault::invalid("invalid model partner catalog")
}
fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        "model partner catalog is unavailable",
    )
}
fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "model partner catalog read cancelled")
}

#[cfg(test)]
mod tests;
