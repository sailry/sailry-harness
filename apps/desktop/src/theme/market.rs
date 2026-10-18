//! Controller-local discovery and verified theme acquisition.
mod catalog;
mod installation;
#[cfg(test)]
mod tests;
mod transfer;

pub(crate) use catalog::{Catalog, Selection};
pub(crate) use installation::{Installed, Prepared};
use sailry_link::CancellationToken;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const INDEX_BYTES: usize = sailry_protocol::MAX_FRAME_BYTES;
use super::package::market::{FILE_BYTES, MANIFEST_BYTES, PACKAGE_BYTES};

#[derive(Clone, Debug)]
pub(crate) struct Listing {
    pub catalog: Catalog,
    pub commit: Option<String>,
    pub unavailable: bool,
}

impl Listing {
    pub fn selection(&self, id: &str) -> Option<Selection> {
        Some(Selection {
            commit: self.commit.clone()?,
            entry: self
                .catalog
                .themes
                .iter()
                .find(|entry| entry.id == id && entry.downloadable())?
                .clone(),
        })
    }
}

#[derive(Clone)]
pub(crate) struct Service {
    root: PathBuf,
    client: reqwest::Client,
    source: transfer::Source,
    cache: Arc<Mutex<Option<Cached>>>,
    publication: Arc<Mutex<()>>,
}

struct Cached {
    listing: Listing,
    until: Instant,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    commit: String,
    catalog: Catalog,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Cancelled,
    Unavailable,
    Invalid,
    Conflict,
    Io(String),
    Publication {
        message: String,
        recovery: Option<PathBuf>,
    },
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("theme operation cancelled"),
            Self::Unavailable => formatter.write_str("theme source is unavailable"),
            Self::Invalid => formatter.write_str("theme source or package is invalid"),
            Self::Conflict => formatter.write_str("installed theme changed"),
            Self::Io(message) => formatter.write_str(message),
            Self::Publication { message, .. } => formatter.write_str(message),
        }
    }
}
impl std::error::Error for Failure {}
impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}
type Result<T> = std::result::Result<T, Failure>;

impl Service {
    pub fn new(root: PathBuf) -> Result<Self> {
        Ok(Self {
            root,
            client: transfer::client()?,
            source: transfer::Source::official(),
            cache: Arc::new(Mutex::new(None)),
            publication: Arc::new(Mutex::new(())),
        })
    }

    pub async fn list(&self, force: bool, stop: &CancellationToken) -> Result<Listing> {
        let mut cached = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(Failure::Cancelled),
            cached = self.cache.lock() => cached,
        };
        if !force && let Some(value) = cached.as_ref().filter(|value| value.until > Instant::now())
        {
            return Ok(value.listing.clone());
        }
        let listing = match self.source.list(&self.client, stop).await {
            Ok(listing) => {
                self.save(&listing)?;
                listing
            }
            Err(Failure::Unavailable) => {
                let mut listing = cached
                    .as_ref()
                    .map(|value| value.listing.clone())
                    .or_else(|| self.restore())
                    .unwrap_or_else(|| Listing {
                        catalog: catalog::bundled(),
                        commit: None,
                        unavailable: true,
                    });
                listing.unavailable = true;
                listing
            }
            Err(error) => return Err(error),
        };
        let lifetime = if listing.unavailable {
            Duration::from_secs(30)
        } else {
            Duration::from_secs(15 * 60)
        };
        *cached = Some(Cached {
            listing: listing.clone(),
            until: Instant::now() + lifetime,
        });
        Ok(listing)
    }

    fn restore(&self) -> Option<Listing> {
        let path = self.root.join(".market/catalog.json");
        let bytes = transfer::read(&path, INDEX_BYTES * 2).ok()?;
        let saved: Saved = serde_json::from_slice(&bytes).ok()?;
        if saved.version != 1
            || !catalog::commit(&saved.commit)
            || saved.catalog.validate().is_err()
        {
            return None;
        }
        Some(Listing {
            catalog: saved.catalog,
            commit: Some(saved.commit),
            unavailable: true,
        })
    }

    fn save(&self, listing: &Listing) -> Result<()> {
        use std::io::Write as _;
        let root = self.root.join(".market");
        std::fs::create_dir_all(&root)?;
        if std::fs::symlink_metadata(&root)?.file_type().is_symlink() {
            return Err(Failure::Invalid);
        }
        let saved = Saved {
            version: 1,
            commit: listing.commit.clone().ok_or(Failure::Invalid)?,
            catalog: listing.catalog.clone(),
        };
        let bytes = serde_json::to_vec(&saved).map_err(|_| Failure::Invalid)?;
        let mut file = tempfile::NamedTempFile::new_in(&root)?;
        file.write_all(&bytes)?;
        file.persist(root.join("catalog.json"))
            .map_err(|error| Failure::Io(error.error.to_string()))?;
        Ok(())
    }
}
