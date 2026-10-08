//! Reference catalog behavior follows Code 67ae9fa0 code_runtime/models_dev.rs.
use super::{invalid, unavailable};
use sailry_protocol::{ErrorCode, Fault};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(crate) mod metadata;
pub(crate) const URL: &str = "https://models.dev/api.json";
const BYTE_LIMIT: usize = 32 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct Catalog {
    source: Source,
    capacity: Arc<Semaphore>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn isolated_refresh_retains_admission_slot() {
        let catalog = Catalog::empty();
        let download = catalog.download().await.unwrap();
        assert!(download.entries.is_empty());
        assert_eq!(download.providers, 0);
        assert_eq!(download.bytes, 2);
        assert!(matches!(catalog.download().await, Err(error) if error.code == ErrorCode::Busy));
        drop(download);
        assert!(catalog.download().await.is_ok());
    }
}

#[derive(Clone)]
enum Source {
    Http(String),
    #[cfg(any(test, feature = "test-support"))]
    Empty,
}

pub(crate) struct Download {
    pub entries: Vec<metadata::Entry>,
    pub bytes: u64,
    pub providers: u32,
    // Keep the refresh slot until the storage transaction completes.
    _permit: OwnedSemaphorePermit,
}

impl Default for Catalog {
    fn default() -> Self {
        Self::new(URL.into())
    }
}

impl Catalog {
    pub(crate) fn new(url: String) -> Self {
        Self {
            source: Source::Http(url),
            capacity: Arc::new(Semaphore::new(1)),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn empty() -> Self {
        Self {
            source: Source::Empty,
            capacity: Arc::new(Semaphore::new(1)),
        }
    }

    pub(crate) async fn download(&self) -> Result<Download, Fault> {
        let permit =
            self.capacity.clone().try_acquire_owned().map_err(|_| {
                Fault::new(ErrorCode::Busy, "model catalog refresh is already running")
            })?;
        let url = match &self.source {
            Source::Http(url) => url,
            #[cfg(any(test, feature = "test-support"))]
            Source::Empty => {
                return Ok(Download {
                    entries: Vec::new(),
                    bytes: 2,
                    providers: 0,
                    _permit: permit,
                });
            }
        };
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|_| unavailable("model catalog HTTP client failed"))?;
        let mut response = client
            .get(url)
            .send()
            .await
            .map_err(|_| unavailable("model catalog download failed"))?;
        if !response.status().is_success() {
            return Err(unavailable("model catalog request was rejected"));
        }
        if response
            .content_length()
            .is_some_and(|length| length > BYTE_LIMIT as u64)
        {
            return Err(unavailable("model catalog exceeds its size limit"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| unavailable("model catalog response failed"))?
        {
            if chunk.len() > BYTE_LIMIT.saturating_sub(bytes.len()) {
                return Err(unavailable("model catalog exceeds its size limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let size = bytes.len() as u64;
        tokio::task::spawn_blocking(move || {
            let (providers, entries) = metadata::parse(&bytes)?;
            Ok(Download {
                entries,
                providers,
                bytes: size,
                _permit: permit,
            })
        })
        .await
        .map_err(|_| unavailable("model catalog parsing failed"))?
    }
}
