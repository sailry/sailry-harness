//! Model discovery follows Code 67ae9fa0 adapter_catalog.rs, with native pagination.
mod authorized;
mod endpoint;
pub(crate) use endpoint::{anthropic as anthropic_base_url, parse as endpoint_url};
pub(crate) mod catalog;
pub(crate) mod cloud;
pub(crate) mod login;
mod metadata;
pub(crate) mod search;
pub(crate) use metadata::validate;

use reqwest::{
    Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, Secret,
    conversation::{ModelApi, discovery},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Semaphore;

const PAGE_BYTES: usize = 2 * 1024 * 1024;
const TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MODEL_LIMIT: usize = 1000;

#[derive(Clone)]
pub(crate) struct Discovery {
    capacity: Arc<Semaphore>,
}

impl Discovery {
    pub(crate) fn new() -> Self {
        Self {
            capacity: Arc::new(Semaphore::new(4)),
        }
    }

    pub(crate) async fn read(
        &self,
        api: ModelApi,
        endpoint: &str,
        key: Option<&Secret>,
        closed: CancellationToken,
    ) -> Result<discovery::Catalog, Fault> {
        if cloud::deployment(api) {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "cloud deployments require an explicit model ID",
            ));
        }
        let candidates = endpoint::candidates(api, endpoint)?;
        self.query(probe(api, candidates, key), closed).await
    }

    async fn query<T>(
        &self,
        query: impl std::future::Future<Output = Result<T, Fault>>,
        closed: CancellationToken,
    ) -> Result<T, Fault> {
        let _permit = self
            .capacity
            .try_acquire()
            .map_err(|_| Fault::new(ErrorCode::Busy, "model discovery capacity exhausted"))?;
        tokio::select! {
            biased;
            _ = closed.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "model discovery cancelled")),
            result = tokio::time::timeout(Duration::from_secs(30), query) => result.map_err(|_| unavailable("model discovery deadline exceeded"))?,
        }
    }
}

async fn probe(
    api: ModelApi,
    candidates: Vec<Url>,
    key: Option<&Secret>,
) -> Result<discovery::Catalog, Fault> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| unavailable("provider HTTP client failed"))?;
    let mut remaining = TOTAL_BYTES;
    for url in candidates {
        let endpoint = url.as_str().trim_end_matches('/').to_owned();
        if let Some(models) = fetch(api, url, key, &client, &mut remaining).await? {
            return Ok(discovery::Catalog { endpoint, models });
        }
    }
    Err(unavailable("provider model endpoint was not found"))
}

async fn fetch(
    api: ModelApi,
    mut url: Url,
    key: Option<&Secret>,
    client: &reqwest::Client,
    remaining: &mut usize,
) -> Result<Option<Vec<discovery::Model>>, Fault> {
    let suffix = if api == ModelApi::Anthropic {
        "/v1/models"
    } else {
        "/models"
    };
    url.set_path(&format!("{}{suffix}", url.path().trim_end_matches('/')));
    let mut headers = HeaderMap::new();
    if let Some(key) = key {
        let (name, value) = match api {
            ModelApi::Anthropic => ("x-api-key", key.expose().to_owned()),
            ModelApi::Gemini => ("x-goog-api-key", key.expose().to_owned()),
            _ => ("authorization", format!("Bearer {}", key.expose())),
        };
        let mut value =
            HeaderValue::from_str(&value).map_err(|_| invalid("provider credential is invalid"))?;
        value.set_sensitive(true);
        headers.insert(HeaderName::from_static(name), value);
    }
    if api == ModelApi::Anthropic {
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    }
    let mut models = BTreeMap::new();
    let mut cursors = BTreeSet::new();
    let mut cursor = None;
    for _ in 0..20 {
        let mut request = client.get(url.clone()).headers(headers.clone());
        match api {
            ModelApi::Anthropic => {
                request = request.query(&[("limit", "100")]);
                if let Some(cursor) = &cursor {
                    request = request.query(&[("after_id", cursor)]);
                }
            }
            ModelApi::Gemini => {
                request = request.query(&[("pageSize", "100")]);
                if let Some(cursor) = &cursor {
                    request = request.query(&[("pageToken", cursor)]);
                }
            }
            _ => {}
        }
        let response = request
            .send()
            .await
            .map_err(|_| unavailable("provider request failed"))?;
        if cursor.is_none()
            && (matches!(response.status().as_u16(), 404 | 405)
                || (response.status().is_success()
                    && response.headers().get("content-type").is_some_and(|value| {
                        value.to_str().is_ok_and(|value| {
                            value
                                .split(';')
                                .next()
                                .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/html"))
                        })
                    })))
        {
            return Ok(None);
        }
        let value = read(response, remaining).await?;
        let page = metadata::parse(api, &value)?;
        for model in page.models {
            if models
                .insert(model.id.clone(), model.clone())
                .is_some_and(|old| old != model)
            {
                return Err(unavailable("provider returned conflicting model metadata"));
            }
            if models.len() > MODEL_LIMIT {
                return Err(unavailable("provider model list exceeds its limit"));
            }
        }
        let Some(next) = page.next else {
            return Ok(Some(models.into_values().collect()));
        };
        if next.is_empty() || next.len() > 2048 || !cursors.insert(next.clone()) {
            return Err(unavailable(
                "provider returned an invalid pagination cursor",
            ));
        }
        cursor = Some(next);
    }
    Err(unavailable("provider model pagination exceeds its limit"))
}

async fn read(
    mut response: reqwest::Response,
    remaining: &mut usize,
) -> Result<serde_json::Value, Fault> {
    if !response.status().is_success() {
        let code = if matches!(response.status().as_u16(), 401 | 403) {
            ErrorCode::NotConfigured
        } else {
            ErrorCode::Unavailable
        };
        return Err(Fault::new(
            code,
            format!("provider returned HTTP {}", response.status().as_u16()),
        ));
    }
    let limit = (*remaining).min(PAGE_BYTES);
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(unavailable("provider response exceeds its size limit"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| unavailable("provider response failed"))?
    {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(unavailable("provider response exceeds its size limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    *remaining -= bytes.len();
    serde_json::from_slice(&bytes).map_err(|_| unavailable("provider returned invalid JSON"))
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}
fn unavailable(message: &str) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}
