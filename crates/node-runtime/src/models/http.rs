use super::{File, METADATA_BYTES};
use reqwest::{Client, StatusCode, Url, header::HeaderMap};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, models::Hub};
use std::time::Duration;

pub(super) struct Http {
    client: Client,
    base: Url,
    #[cfg(any(test, feature = "test-support"))]
    fixture: bool,
}

pub(super) struct Response {
    pub body: Vec<u8>,
    pub headers: HeaderMap,
}

impl Http {
    pub fn new(hub: Hub, mirror: Option<&str>) -> Result<Self, Fault> {
        let base = match hub {
            Hub::HuggingFace => mirror.unwrap_or("https://huggingface.co/"),
            Hub::ModelScope => "https://modelscope.cn/",
        };
        let base = Url::parse(base).map_err(|_| Fault::invalid("invalid model Hub URL"))?;
        if base.scheme() != "https"
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(Fault::invalid(
                "model Hub requires HTTPS without credentials",
            ));
        }
        Ok(Self {
            client: client(true)?,
            base,
            #[cfg(any(test, feature = "test-support"))]
            fixture: false,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn fixture(base: Url) -> Self {
        Self {
            client: client(false).expect("fixture client is valid"),
            base,
            fixture: true,
        }
    }

    pub fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("Hub URL can contain path segments")
            .pop_if_empty()
            .extend(segments.iter().copied());
        url
    }

    pub fn repository(&self, prefix: &[&str], repository: &str, suffix: &[&str]) -> Url {
        let mut segments = prefix.to_vec();
        segments.extend(repository.split('/'));
        segments.extend(suffix);
        self.url(&segments)
    }

    pub fn file(&self, file: &File) -> Result<Url, Fault> {
        let asset = sailry_protocol::models::Asset {
            source: file.source.clone(),
            path: file.path.clone(),
            name: "metadata.json".into(),
            bytes: file.bytes,
            sha256: file.sha256.clone().unwrap_or_default(),
        };
        let base = self.base.clone();
        #[cfg(any(test, feature = "test-support"))]
        let base = if self.fixture {
            let mut base = base;
            base.set_scheme("https")
                .expect("fixture URL supports HTTPS");
            base
        } else {
            base
        };
        let url = sailry_model_assets::url(&asset, Some(base.as_str()))
            .map_err(|_| Fault::invalid("invalid pinned model asset URL"))?;
        #[cfg(any(test, feature = "test-support"))]
        if self.fixture {
            let mut fixture = self.base.clone();
            fixture.set_path(url.path());
            fixture.set_query(url.query());
            return Ok(fixture);
        }
        Ok(url)
    }

    pub async fn json(
        &self,
        url: Url,
        cancel: &CancellationToken,
    ) -> Result<(serde_json::Value, HeaderMap), Fault> {
        let response = tokio::time::timeout(
            Duration::from_secs(60),
            self.get(url, METADATA_BYTES, cancel),
        )
        .await
        .map_err(|_| unavailable())??;
        let value = serde_json::from_slice(&response.body)
            .map_err(|_| Fault::invalid("model catalog returned invalid JSON"))?;
        Ok((value, response.headers))
    }

    pub async fn get(
        &self,
        url: Url,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Response, Fault> {
        let mut response = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            response = self.client.get(url).send() => response.map_err(|_| unavailable())?,
        };
        if !response.status().is_success() {
            return Err(match response.status() {
                StatusCode::NOT_FOUND => Fault::new(
                    ErrorCode::NotFound,
                    "model repository or revision was not found",
                ),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Fault::new(
                    ErrorCode::PermissionDenied,
                    "model repository requires authentication",
                ),
                _ => unavailable(),
            });
        }
        if response
            .content_length()
            .is_some_and(|bytes| bytes > limit as u64)
        {
            return Err(Fault::invalid(
                "model catalog response exceeds its size limit",
            ));
        }
        let headers = response.headers().clone();
        let mut body = Vec::new();
        loop {
            let chunk = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(cancelled()),
                chunk = response.chunk() => chunk.map_err(|_| unavailable())?,
            };
            let Some(chunk) = chunk else { break };
            if body.len().saturating_add(chunk.len()) > limit {
                return Err(Fault::invalid(
                    "model catalog response exceeds its size limit",
                ));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Response { body, headers })
    }
}

fn client(https: bool) -> Result<Client, Fault> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let builder = Client::builder();
    let builder = if https { builder } else { builder.no_proxy() };
    builder
        .https_only(https)
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("Sailry model catalog")
        .build()
        .map_err(|_| unavailable())
}

pub(super) fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "model catalog request cancelled")
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "model catalog request failed")
}
