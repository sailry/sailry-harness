//! Node owns HTTP, credential injection and the configured endpoint trust boundary.
use reqwest::{
    Client, Response,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, Secret,
    plugin::model_adapter::{Declaration, wire::Prepared},
};
use std::collections::BTreeMap;
use url::Url;

pub(super) const REQUEST_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug)]
pub(super) enum Failure {
    Request(Fault),
    Network(reqwest::Error),
}

impl From<Fault> for Failure {
    fn from(fault: Fault) -> Self {
        Self::Request(fault)
    }
}

impl Failure {
    pub(super) fn fault(self) -> Fault {
        match self {
            Self::Request(fault) => fault,
            Self::Network(_) => network(),
        }
    }

    pub(super) fn model(self) -> goose_provider_types::errors::ProviderError {
        match self {
            Self::Request(fault) => super::response::fault(fault),
            Self::Network(_) => goose_provider_types::errors::ProviderError::NetworkError(
                "model connection failed".into(),
            ),
        }
    }
}

#[derive(Clone)]
pub(super) struct Credentials {
    pub(super) headers: HeaderMap,
    pub(super) access: Option<Secret>,
    pub(super) key: Option<Secret>,
    pub(super) secrets: Vec<Secret>,
}

pub(super) fn public(value: &str, credentials: &Credentials) -> Result<(), Fault> {
    if credentials
        .secrets
        .iter()
        .any(|secret| !secret.expose().is_empty() && value.contains(secret.expose()))
    {
        Err(Fault::new(
            ErrorCode::PermissionDenied,
            "model response exposed a private credential",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn client() -> Result<Client, Fault> {
    Client::builder()
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(
            goose_providers::api_client::DEFAULT_PROVIDER_TIMEOUT_SECS,
        ))
        .build()
        .map_err(|_| Fault::new(ErrorCode::Unavailable, "model HTTP client is unavailable"))
}

pub(super) async fn send(
    client: &Client,
    prepared: Prepared,
    endpoint: &str,
    declaration: &Declaration,
    credentials: &Credentials,
    stop: &CancellationToken,
    budget: usize,
) -> Result<Response, Failure> {
    let budget = budget.min(REQUEST_BYTES);
    let endpoint = address(endpoint)?;
    let url = address(&prepared.url)?;
    let same_origin = url.origin() == endpoint.origin();
    if !same_origin
        && !declaration
            .origins
            .iter()
            .any(|origin| address(origin).is_ok_and(|origin| origin.origin() == url.origin()))
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "model request origin was not declared",
        )
        .into());
    }
    let mut headers = HeaderMap::new();
    let mut bytes = prepared
        .url
        .len()
        .checked_add(prepared.body.as_ref().map_or(0, String::len))
        .filter(|bytes| *bytes <= budget)
        .ok_or_else(resource)?;
    for (name, value) in prepared.headers {
        bytes = bytes
            .checked_add(name.len())
            .and_then(|bytes| bytes.checked_add(value.len()))
            .filter(|bytes| *bytes <= budget)
            .ok_or_else(resource)?;
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid())?;
        if forbidden(name.as_str()) {
            return Err(invalid().into());
        }
        let value = HeaderValue::from_str(&value).map_err(|_| invalid())?;
        headers.try_insert(name, value).map_err(|_| invalid())?;
    }
    if same_origin {
        for (name, value) in &credentials.headers {
            headers
                .try_insert(name.clone(), value.clone())
                .map_err(|_| invalid())?;
        }
    }
    if let Some(injection) = prepared.credential {
        if !same_origin {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "model credentials cannot leave the configured origin",
            )
            .into());
        }
        let name = HeaderName::from_bytes(injection.header.as_bytes()).map_err(|_| invalid())?;
        if forbidden(name.as_str()) {
            return Err(invalid().into());
        }
        let mut value = injection.template;
        for (token, secret) in [
            ("{access}", &credentials.access),
            ("{key}", &credentials.key),
        ] {
            if value.contains(token) {
                let secret = secret.as_ref().ok_or_else(|| {
                    Fault::new(ErrorCode::NotConfigured, "model credential is unavailable")
                })?;
                value = value.replace(token, secret.expose());
            }
        }
        if value.contains('{') || value.contains('}') {
            return Err(invalid().into());
        }
        let mut value = HeaderValue::from_str(&value).map_err(|_| invalid())?;
        value.set_sensitive(true);
        headers.try_insert(name, value).map_err(|_| invalid())?;
    }
    let header_bytes = headers
        .iter()
        .try_fold(0usize, |bytes, (name, value)| {
            bytes
                .checked_add(name.as_str().len())
                .and_then(|bytes| bytes.checked_add(value.as_bytes().len()))
        })
        .ok_or_else(resource)?;
    if prepared
        .url
        .len()
        .checked_add(prepared.body.as_ref().map_or(0, String::len))
        .and_then(|bytes| bytes.checked_add(header_bytes))
        .is_none_or(|bytes| bytes > budget)
    {
        return Err(resource().into());
    }
    let method = match prepared.method {
        sailry_protocol::plugin::model_auth::Method::Get => reqwest::Method::GET,
        sailry_protocol::plugin::model_auth::Method::Post => reqwest::Method::POST,
    };
    let mut request = client.request(method, url).headers(headers);
    if let Some(body) = prepared.body {
        request = request.body(body);
    }
    let request = request.build().map_err(Failure::Network)?;
    tokio::select! {
        biased;
        _ = stop.cancelled() => Err(cancelled().into()),
        response = client.execute(request) => response.map_err(Failure::Network),
    }
}

// Adapted from adk-model 675c02b6, Apache-2.0 (third_party_licenses/adk-LICENSE).
// Only the RFC Retry-After parser is retained; retry policy belongs to GDK.
pub(super) fn retry_after(headers: &HeaderMap) -> Option<std::time::Duration> {
    let value = headers
        .get("retry-after")?
        .to_str()
        .ok()?
        .trim_matches([' ', '\t']);
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        let delay = std::time::Duration::from_secs(value.parse().ok()?);
        u64::try_from(delay.as_millis()).ok()?;
        return Some(delay);
    }
    let date = httpdate::parse_http_date(value).ok()?;
    let delay = date
        .duration_since(std::time::SystemTime::now())
        .unwrap_or_default();
    let millis = delay.as_nanos().div_ceil(1_000_000);
    u64::try_from(millis)
        .ok()
        .map(std::time::Duration::from_millis)
}

pub(super) fn headers(
    headers: &HeaderMap,
    credentials: &Credentials,
) -> Result<BTreeMap<String, Vec<String>>, Fault> {
    // Package decoders never receive cookies, authentication or arbitrary
    // upstream headers, even when the response was sent by an approved origin.
    let mut values = BTreeMap::new();
    for name in ["content-type", "retry-after"] {
        let entries: Vec<_> = headers
            .get_all(name)
            .iter()
            .filter_map(|value| value.to_str().ok().map(str::to_owned))
            .collect();
        for value in &entries {
            public(value, credentials)?;
        }
        if !entries.is_empty() {
            values.insert(name.into(), entries);
        }
    }
    Ok(values)
}

fn address(value: &str) -> Result<Url, Fault> {
    let url = Url::parse(value).map_err(|_| invalid())?;
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !(url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
    {
        return Err(invalid());
    }
    Ok(url)
}

fn forbidden(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "connection"
            | "content-length"
            | "transfer-encoding"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "upgrade"
            | "trailer"
    )
}
fn invalid() -> Fault {
    Fault::invalid("model request has an invalid URL or header")
}
fn resource() -> Fault {
    Fault::new(
        ErrorCode::Internal,
        "model adapter resource budget exceeded",
    )
}
pub(super) fn network() -> Fault {
    Fault::new(ErrorCode::Unavailable, "model connection failed")
}
pub(super) fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "model request was cancelled")
}
