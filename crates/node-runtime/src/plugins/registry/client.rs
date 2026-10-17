use super::*;
use reqwest::Url;
use serde::Deserialize;
use std::{
    io::{Seek, Write},
    time::Duration,
};

#[derive(Clone, Default)]
pub(crate) struct Registry {
    #[cfg(any(test, feature = "test-support"))]
    endpoint: std::sync::Arc<std::sync::Mutex<Option<Url>>>,
}

#[derive(Deserialize)]
struct Detail {
    namespace: Namespace,
    skill: Skill,
    #[serde(rename = "latestVersion")]
    latest: Latest,
}
#[derive(Deserialize)]
struct Namespace {
    handle: String,
}
#[derive(Deserialize)]
struct Skill {
    slug: String,
}
#[derive(Deserialize)]
struct Latest {
    version: String,
}

impl Registry {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_fixture(&self, endpoint: &str) -> Result<(), Fault> {
        let url = Url::parse(endpoint).map_err(|_| invalid())?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        *self
            .endpoint
            .lock()
            .expect("registry fixture lock is valid") = Some(url);
        Ok(())
    }

    fn url(&self, parts: &[&str]) -> Url {
        #[cfg(any(test, feature = "test-support"))]
        let fixture = self
            .endpoint
            .lock()
            .expect("registry fixture lock is valid")
            .clone();
        #[cfg(not(any(test, feature = "test-support")))]
        let fixture: Option<Url> = None;
        let mut url = fixture.unwrap_or_else(|| Url::parse("https://api.skillhub.cn").unwrap());
        url.path_segments_mut().unwrap().clear().extend(parts);
        url
    }

    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        url: Url,
        stop: &CancellationToken,
    ) -> Result<T, Fault> {
        let mut file = self.download(url, 1024 * 1024, false, stop).await?;
        file.rewind().map_err(io_error)?;
        serde_json::from_reader(file).map_err(|_| invalid())
    }

    async fn download(
        &self,
        mut url: Url,
        limit: u64,
        redirects: bool,
        stop: &CancellationToken,
    ) -> Result<File, Fault> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .user_agent("Sailry")
            .build()
            .map_err(|_| unavailable())?;
        let operation = async {
            for hop in 0..=5 {
                let mut response = client
                    .get(url.clone())
                    .send()
                    .await
                    .map_err(|_| unavailable())?;
                if response.status().is_redirection() && redirects && hop < 5 {
                    let location = response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|value| value.to_str().ok())
                        .ok_or_else(invalid)?;
                    let next = url.join(location).map_err(|_| invalid())?;
                    if !self.download_url(&next) {
                        return Err(invalid());
                    }
                    url = next;
                    continue;
                }
                if !response.status().is_success() {
                    return Err(Fault::new(
                        if response.status() == reqwest::StatusCode::NOT_FOUND {
                            ErrorCode::NotFound
                        } else {
                            ErrorCode::Unavailable
                        },
                        "Skillhub source is unavailable",
                    ));
                }
                if response.content_length().is_some_and(|size| size > limit) {
                    return Err(invalid());
                }
                let mut file = tempfile::tempfile().map_err(io_error)?;
                let mut size = 0u64;
                while let Some(bytes) = response.chunk().await.map_err(|_| unavailable())? {
                    size = size
                        .checked_add(bytes.len() as u64)
                        .filter(|size| *size <= limit)
                        .ok_or_else(invalid)?;
                    file.write_all(&bytes).map_err(io_error)?;
                }
                file.rewind().map_err(io_error)?;
                return Ok(file);
            }
            Err(invalid())
        };
        tokio::select! { _ = stop.cancelled() => Err(super::super::github::cancelled()), result = tokio::time::timeout(Duration::from_secs(60), operation) => result.map_err(|_| unavailable())? }
    }

    fn download_url(&self, url: &Url) -> bool {
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return false;
        }
        #[cfg(any(test, feature = "test-support"))]
        if self
            .endpoint
            .lock()
            .expect("registry fixture lock is valid")
            .as_ref()
            .is_some_and(|endpoint| endpoint.origin() == url.origin())
        {
            return true;
        }
        url.scheme() == "https"
            && url.port_or_known_default() == Some(443)
            && url
                .host_str()
                .is_some_and(|host| host == "api.skillhub.cn" || host.ends_with(".myqcloud.com"))
    }

    pub(super) async fn acquire(
        &self,
        source: &RegistrySource,
        expected: Option<&RegistryResolved>,
        stop: &CancellationToken,
    ) -> Result<(File, RegistryResolved), Fault> {
        validate(source)?;
        let mut url = self.url(&["api", "v1", "skills", &source.slug]);
        if let Some(namespace) = &source.namespace {
            url.query_pairs_mut().append_pair("namespace", namespace);
        }
        let detail: Detail = self.json(url, stop).await?;
        if !segment(&detail.namespace.handle)
            || detail.skill.slug != source.slug
            || source
                .namespace
                .as_ref()
                .is_some_and(|namespace| *namespace != detail.namespace.handle)
        {
            return Err(invalid());
        }
        let namespace = detail.namespace.handle;
        let version = source
            .version
            .as_ref()
            .unwrap_or(&detail.latest.version)
            .clone();
        semver::Version::parse(&version).map_err(|_| invalid())?;
        let mut url = self.url(&[
            "api",
            "v1",
            "open",
            "skills",
            &source.slug,
            "versions",
            &version,
            "signature",
        ]);
        url.query_pairs_mut().append_pair("namespace", &namespace);
        let signature: verification::Envelope = self.json(url, stop).await?;
        let keys: verification::Keys = self
            .json(self.url(&["api", "v1", "open", "platform", "keys"]), stop)
            .await?;
        let payload = verification::signature(&signature, &keys, &source.slug, &version)?;
        let mut url = self.url(&["api", "v1", "download"]);
        url.query_pairs_mut()
            .append_pair("slug", &source.slug)
            .append_pair("namespace", &namespace)
            .append_pair("version", &version);
        let file = self
            .download(url, sailry_protocol::plugin::MAX_PACKAGE_BYTES, true, stop)
            .await?;
        let stop = stop.clone();
        let resolved = RegistryResolved {
            namespace,
            slug: source.slug.clone(),
            version,
            version_id: payload.version_id.to_string(),
            content_hash: payload.content_hash.clone(),
            package_md5: payload.package_md5.clone(),
            archive_sha256: String::new(),
        };
        let (file, resolved) = tokio::task::spawn_blocking(move || {
            verification::archive(file, resolved, &payload, &stop)
        })
        .await
        .map_err(|_| unavailable())??;
        if expected.is_some_and(|expected| *expected != resolved) {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "Skillhub package changed; discover it again",
            ));
        }
        Ok((file, resolved))
    }
}

pub(super) fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "Skillhub source is unavailable")
}
