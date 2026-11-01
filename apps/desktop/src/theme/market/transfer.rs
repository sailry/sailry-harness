use super::{Failure, INDEX_BYTES, Listing, Result, SourceFailure, catalog};
use sailry_link::CancellationToken;
use std::{io::Read as _, path::Path, time::Duration};

#[derive(Clone)]
pub(super) struct Source {
    pub catalog: String,
    pub raw: String,
}

impl Source {
    pub fn official() -> Self {
        Self {
            catalog: "https://sailry.dev/themes/catalog.json".into(),
            raw: "https://raw.githubusercontent.com/sailry/sailry-themes/".into(),
        }
    }

    pub async fn list(
        &self,
        client: &reqwest::Client,
        stop: &CancellationToken,
    ) -> Result<Listing> {
        let bytes = body(client, &self.catalog, INDEX_BYTES, stop).await?;
        #[derive(serde::Deserialize)]
        struct Feed {
            source_commit: String,
            #[serde(flatten)]
            catalog: catalog::Catalog,
        }
        let feed: Feed = serde_json::from_slice(&bytes).map_err(|_| Failure::Invalid)?;
        if !catalog::commit(&feed.source_commit) {
            return Err(Failure::Invalid);
        }
        feed.catalog.validate()?;
        Ok(Listing {
            catalog: feed.catalog,
            commit: Some(feed.source_commit),
            unavailable: false,
        })
    }

    pub fn resource(&self, commit: &str, entry: &catalog::Entry, path: &str) -> String {
        format!("{}{}/{}/{}", self.raw, commit, entry.path, path)
    }
}

pub(super) fn client() -> Result<reqwest::Client> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .referer(false)
        .user_agent(concat!("Sailry/", env!("CARGO_PKG_VERSION")))
        // Official catalog and raw resource URLs do not need cross-origin redirects.
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .build()
        .map_err(request_failure)
}

fn request_failure(error: reqwest::Error) -> Failure {
    unavailable(if error.is_timeout() {
        SourceFailure::Timeout
    } else {
        SourceFailure::Network
    })
}

fn unavailable(reason: SourceFailure) -> Failure {
    let failure = Failure::Unavailable(reason);
    eprintln!("could not load theme source: {failure}");
    failure
}

pub(super) async fn body(
    client: &reqwest::Client,
    url: &str,
    limit: usize,
    stop: &CancellationToken,
) -> Result<Vec<u8>> {
    let request = async {
        let mut response = client
            .get(url)
            .timeout(Duration::from_secs(45))
            .send()
            .await
            .map_err(request_failure)?;
        if !response.status().is_success() {
            return Err(unavailable(SourceFailure::Http(response.status().as_u16())));
        }
        if response
            .content_length()
            .is_some_and(|bytes| bytes > limit as u64)
        {
            return Err(Failure::Invalid);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(request_failure)? {
            if bytes.len().saturating_add(chunk.len()) > limit {
                return Err(Failure::Invalid);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    };
    tokio::select! {
        biased;
        _ = stop.cancelled() => Err(Failure::Cancelled),
        result = request => result,
    }
}

pub(super) fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit as u64 {
        return Err(Failure::Invalid);
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(Failure::Invalid);
    }
    Ok(bytes)
}
