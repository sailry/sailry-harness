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

    pub(super) fn model(self) -> adk_core::AdkError {
        match self {
            Self::Request(fault) => super::response::fault(fault),
            Self::Network(error) => {
                let (category, code, message) = if error.is_timeout() {
                    (
                        adk_core::ErrorCategory::Timeout,
                        "model.plugin.timeout",
                        "model HTTP request timed out",
                    )
                } else if error.is_body() {
                    (
                        adk_core::ErrorCategory::Unavailable,
                        "model.plugin.stream_read",
                        "model response stream could not be read",
                    )
                } else {
                    (
                        adk_core::ErrorCategory::Unavailable,
                        "model.plugin.connection",
                        "model connection failed",
                    )
                };
                let kind = if error.is_timeout() {
                    "timeout"
                } else if error.is_connect() {
                    "connection"
                } else if error.is_builder() {
                    "build"
                } else {
                    "network"
                };
                let mut failure = adk_core::AdkError::new(
                    adk_core::ErrorComponent::Model,
                    category,
                    code,
                    message,
                )
                .with_source(Fault::new(ErrorCode::Unavailable, message));
                failure
                    .details
                    .metadata
                    .insert("transport".into(), "http".into());
                failure.details.metadata.insert(
                    "transport_phase".into(),
                    if error.is_body() { "stream" } else { "request" }.into(),
                );
                failure
                    .details
                    .metadata
                    .insert("transport_error_kind".into(), kind.into());
                failure
            }
        }
    }
}

pub(super) struct Credentials {
    pub(super) headers: HeaderMap,
    pub(super) access: Option<Secret>,
    pub(super) key: Option<Secret>,
    pub(super) secrets: Vec<Secret>,
    pub(super) origin: Option<adk_core::ProviderOrigin>,
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
        .read_timeout(std::time::Duration::from_secs(300))
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
    retries: u64,
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
    for attempt in 0..=retries {
        let sent = request.try_clone().ok_or_else(invalid)?;
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(cancelled().into()),
            response = client.execute(sent) => response,
        };
        let retry = match &result {
            Ok(response) => response.status().is_server_error(),
            Err(error) => !error.is_builder(),
        };
        if attempt == retries || !retry {
            return result.map_err(Failure::Network);
        }
        let at = result
            .as_ref()
            .ok()
            .and_then(|response| deadline(response.headers()))
            .unwrap_or_else(|| {
                tokio::time::Instant::now() + super::super::retry::budget::backoff(attempt + 1)
            });
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(cancelled().into()),
            _ = tokio::time::sleep_until(at) => {},
        }
    }
    unreachable!("inclusive retry loop returns its last result")
}

pub(super) fn deadline(headers: &HeaderMap) -> Option<tokio::time::Instant> {
    let delay = adk_model::retry::parse_retry_after(headers.get("retry-after")?.to_str().ok()?)?;
    tokio::time::Instant::now().checked_add(delay)
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
