//! Revision-bound building blocks for package-declared model authentication.
//! The login worker owns admission, persistence, progress and final binding checks.
use reqwest::{
    RequestBuilder, Url,
    header::{HeaderName, HeaderValue},
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, ProviderId, RequestId, Secret,
    plugin::{Reference, model_auth},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) mod flow;
mod identity;
pub(crate) mod workflow;

#[cfg(test)]
mod tests;

/// Captured before starting a login. Completion must compare against live state
/// again in the existing credential transaction, rather than resolving latest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Capture {
    pub attempt: RequestId,
    pub provider: ProviderId,
    pub revision: u64,
    pub package: Reference,
    pub origin: sailry_protocol::conversation::consent::Origin,
    pub flow: String,
}

impl Capture {
    pub(crate) fn check(
        &self,
        attempt: RequestId,
        provider: ProviderId,
        revision: u64,
        package: &Reference,
        origin: &sailry_protocol::conversation::consent::Origin,
        flow: &str,
    ) -> Result<(), Fault> {
        if attempt != self.attempt || provider != self.provider || flow != self.flow {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "model authorization belongs to another attempt",
            ));
        }
        if revision != self.revision || package != &self.package || origin != &self.origin {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "model authorization configuration changed",
            ));
        }
        Ok(())
    }
}

/// Private values are never used as public callback results or progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Issued {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
    pub access: Secret,
    pub refresh: Option<Secret>,
    pub expires_at_ms: Option<u64>,
    pub extra: BTreeMap<String, Secret>,
}

/// Account ownership is stable across implementation-only package updates.
/// Private client values participate in equality without entering public data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Target {
    pub provider: ProviderId,
    pub package: String,
    /// Retained integrity metadata; it does not approve or invalidate an account.
    #[serde(default)]
    pub digest: Option<String>,
    pub origin: sailry_protocol::conversation::consent::Origin,
    pub flow: String,
    pub endpoint: String,
    pub authorization: model_auth::Flow,
    pub refresh: Option<model_auth::Endpoint>,
    pub inject: Vec<model_auth::Injection>,
    pub fields: BTreeMap<String, Secret>,
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Record {
    pub target: Target,
    pub issued: Issued,
}

impl Record {
    pub(crate) fn validate(&self) -> Result<(), Fault> {
        if self.target.package.is_empty()
            || self.target.digest.as_ref().is_some_and(|digest| {
                digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            || self.target.flow.is_empty()
            || self.issued.access.expose().is_empty()
            || self
                .issued
                .refresh
                .as_ref()
                .is_some_and(|value| value.expose().is_empty())
            || self
                .issued
                .expires_at_ms
                .is_some_and(|value| value == 0 || value > i64::MAX as u64)
        {
            return Err(invalid());
        }
        self.issued.headers(&self.target.inject)?;
        Ok(())
    }

    pub(crate) fn matches(&self, target: &Target) -> Result<(), Fault> {
        self.target.matches(target)
    }
}

impl Target {
    pub(crate) fn refreshable(&self) -> bool {
        self.refresh.is_some()
            || matches!(
                self.authorization,
                model_auth::Flow::Script { refresh: true, .. }
            )
    }

    pub(crate) fn matches(&self, target: &Target) -> Result<(), Fault> {
        let previous = self;
        if previous.provider != target.provider
            || previous.package != target.package
            || previous.flow != target.flow
            || previous.endpoint != target.endpoint
            || previous.authorization != target.authorization
            || previous.refresh != target.refresh
            || previous.inject != target.inject
            || previous.fields != target.fields
            || previous.scopes != target.scopes
        {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "model authentication changed; sign in again",
            ));
        }
        Ok(())
    }
}

impl Issued {
    pub(crate) fn parse(
        document: &Value,
        mapping: &model_auth::Mapping,
        now_ms: u64,
        previous: Option<&Self>,
    ) -> Result<Self, Fault> {
        let access = text(document, &mapping.access)?.ok_or_else(invalid)?;
        let refresh = mapping
            .refresh
            .as_ref()
            .map(|pointer| text(document, pointer))
            .transpose()?
            .flatten()
            .or_else(|| previous.and_then(|value| value.refresh.clone()));
        let expires_at_ms = match mapping
            .expires_in
            .as_ref()
            .and_then(|pointer| document.pointer(pointer))
        {
            Some(Value::Number(value)) => {
                let seconds = value
                    .as_u64()
                    .filter(|value| *value > 0)
                    .ok_or_else(invalid)?;
                Some(
                    seconds
                        .checked_mul(1000)
                        .and_then(|value| now_ms.checked_add(value))
                        .ok_or_else(invalid)?,
                )
            }
            Some(_) => return Err(invalid()),
            None if mapping.expires_in.is_some() => return Err(invalid()),
            None => None,
        };
        let mut extra = previous
            .map(|value| value.extra.clone())
            .unwrap_or_default();
        for (key, pointer) in &mapping.extra {
            if let Some(value) = text(document, pointer)? {
                extra.insert(key.clone(), value);
            }
        }
        let account_label = mapping
            .account_label
            .as_ref()
            .and_then(|pointer| document.pointer(pointer))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| {
                !value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control)
            })
            .map(str::to_owned)
            .or_else(|| previous.and_then(|value| value.account_label.clone()));
        Ok(Self {
            account_label,
            access,
            refresh,
            expires_at_ms,
            extra,
        })
    }

    pub(crate) fn fields(&self) -> BTreeMap<String, Secret> {
        let mut fields = BTreeMap::from([("access".into(), self.access.clone())]);
        if let Some(refresh) = &self.refresh {
            fields.insert("refresh".into(), refresh.clone());
        }
        for (key, value) in &self.extra {
            fields.insert(format!("extra.{key}"), value.clone());
        }
        fields
    }

    pub(crate) fn headers(
        &self,
        declarations: &[model_auth::Injection],
    ) -> Result<reqwest::header::HeaderMap, Fault> {
        let fields = self.fields();
        let mut headers = reqwest::header::HeaderMap::new();
        for declaration in declarations {
            let (name, value) = header(&declaration.header, &declaration.template, &fields)?;
            if headers
                .try_insert(name, value)
                .map_err(|_| invalid())?
                .is_some()
            {
                return Err(invalid());
            }
        }
        Ok(headers)
    }
}

/// A single declared request. This does not poll, retry, or persist a result.
/// Flows compose it through the existing login worker and cancellation owner.
pub(crate) async fn request(
    service: &super::Service,
    endpoint: &model_auth::Endpoint,
    fields: &BTreeMap<String, Secret>,
    stop: &CancellationToken,
) -> Result<Value, Fault> {
    let (status, document) = response(service, endpoint, fields, stop).await?;
    if rejected(status, &document) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "authorization was rejected; sign in again",
        ));
    }
    if !status.is_success() {
        return Err(super::unavailable());
    }
    Ok(document)
}

async fn refresh_request(
    service: &super::Service,
    endpoint: &model_auth::Endpoint,
    fields: &BTreeMap<String, Secret>,
    stop: &CancellationToken,
) -> Result<Value, Fault> {
    let (status, document) = response_with(service, endpoint, fields, stop, true, false).await?;
    if rejected(status, &document) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "authorization refresh was rejected; sign in again",
        ));
    }
    if !status.is_success() {
        return Err(uncertain());
    }
    Ok(document)
}

fn rejected(status: reqwest::StatusCode, document: &Value) -> bool {
    status.as_u16() == 401
        || !status.is_server_error()
            && document.get("error").and_then(Value::as_str) == Some("invalid_grant")
}

fn uncertain() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "authorization refresh did not complete; sign in again",
    )
}

async fn response(
    service: &super::Service,
    endpoint: &model_auth::Endpoint,
    fields: &BTreeMap<String, Secret>,
    stop: &CancellationToken,
) -> Result<(reqwest::StatusCode, Value), Fault> {
    response_with(service, endpoint, fields, stop, false, false).await
}

async fn response_with(
    service: &super::Service,
    endpoint: &model_auth::Endpoint,
    fields: &BTreeMap<String, Secret>,
    stop: &CancellationToken,
    renewal: bool,
    error_document: bool,
) -> Result<(reqwest::StatusCode, Value), Fault> {
    if !endpoint.valid() {
        return Err(invalid());
    }
    let client = super::http_client("Sailry/0.1")?;
    // Reuse the native login fixture's transport seam. Production validation and
    // persisted targets always retain the declaration's original HTTPS URL.
    #[cfg(any(test, feature = "test-support"))]
    let routed = service
        .model_endpoint
        .as_ref()
        .map(|base| {
            let source = Url::parse(&endpoint.url).map_err(|_| invalid())?;
            let mut target = Url::parse(base).map_err(|_| invalid())?;
            target.set_path(source.path());
            target.set_query(source.query());
            let mut routed = endpoint.clone();
            routed.url = target.into();
            Ok::<_, Fault>(routed)
        })
        .transpose()?;
    #[cfg(any(test, feature = "test-support"))]
    let endpoint = routed.as_ref().unwrap_or(endpoint);
    #[cfg(not(any(test, feature = "test-support")))]
    let _ = service;
    let request = prepare(&client, endpoint, fields)?;
    tokio::select! {
        biased;
        _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "model authorization cancelled")),
        result = async {
            if !renewal {
                return super::response(request).await.map_err(|error| {
                    if error.code == ErrorCode::PermissionDenied {
                        super::unavailable()
                    } else {
                        error
                    }
                });
            }
            let mut response=request.send().await.map_err(|error| if error.is_connect() {
                Fault::new(ErrorCode::Unavailable,"authorization endpoint could not be reached")
            } else {uncertain()})?;
            let status=response.status();
            let mut body=zeroize::Zeroizing::new(Vec::new());
            while let Some(chunk)=response.chunk().await.map_err(|_|uncertain())? {
                if chunk.len()>super::BODY_LIMIT.saturating_sub(body.len()) {return Err(uncertain())}
                body.extend_from_slice(&chunk);
            }
            Ok((status,body))
        } => {
            let (status, body) = result?;
            if status.as_u16() == 401 {return Ok((status,Value::Null))}
            match serde_json::from_slice(&body) { Ok(document)=>Ok((status,document)), Err(_) if error_document && !status.is_success()=>Ok((status,Value::Null)), Err(_)=>Err(invalid()) }
        }
    }
}

pub(crate) fn prepare(
    client: &reqwest::Client,
    endpoint: &model_auth::Endpoint,
    fields: &BTreeMap<String, Secret>,
) -> Result<RequestBuilder, Fault> {
    let url = Url::parse(&endpoint.url).map_err(|_| invalid())?;
    let request = match endpoint.method {
        model_auth::Method::Get => client.get(url),
        model_auth::Method::Post => client.post(url),
    };
    let mut request = request;
    for (name, value) in &endpoint.headers {
        let (name, value) = header(name, value, fields)?;
        request = request.header(name, value);
    }
    let query = endpoint
        .query
        .iter()
        .map(|(key, value)| Ok((key, expand(value, fields)?)))
        .collect::<Result<Vec<_>, Fault>>()?;
    request = request.query(&query);
    if let Some(body) = &endpoint.body {
        request = match body {
            model_auth::Body::Form { fields: body } => {
                let body = body
                    .iter()
                    .map(|(key, value)| Ok((key, expand(value, fields)?)))
                    .collect::<Result<Vec<_>, Fault>>()?;
                request.form(&body)
            }
            model_auth::Body::Json { value } => request.json(&json(value, fields)?),
        };
    }
    Ok(request)
}

fn header(
    name: &str,
    template: &str,
    fields: &BTreeMap<String, Secret>,
) -> Result<(HeaderName, HeaderValue), Fault> {
    let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid())?;
    if matches!(
        name.as_str(),
        "host"
            | "connection"
            | "content-length"
            | "transfer-encoding"
            | "proxy-authorization"
            | "upgrade"
    ) {
        return Err(invalid());
    }
    let mut value = HeaderValue::from_str(&expand(template, fields)?).map_err(|_| invalid())?;
    value.set_sensitive(true);
    Ok((name, value))
}

fn json(value: &Value, fields: &BTreeMap<String, Secret>) -> Result<Value, Fault> {
    match value {
        Value::String(value) => expand(value, fields).map(Value::String),
        Value::Array(values) => values.iter().map(|value| json(value, fields)).collect(),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| Ok((key.clone(), json(value, fields)?)))
            .collect::<Result<serde_json::Map<_, _>, Fault>>()
            .map(Value::Object),
        value => Ok(value.clone()),
    }
}

fn expand(template: &str, fields: &BTreeMap<String, Secret>) -> Result<String, Fault> {
    let mut rest = template;
    let mut result = String::new();
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        let part = &rest[start + 1..];
        let end = part.find('}').ok_or_else(invalid)?;
        let value = fields.get(&part[..end]).ok_or_else(invalid)?;
        result.push_str(value.expose());
        rest = &part[end + 1..];
    }
    result.push_str(rest);
    Ok(result)
}

fn text(document: &Value, pointer: &str) -> Result<Option<Secret>, Fault> {
    match document.pointer(pointer) {
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(Secret::new(value.clone()))),
        None => Ok(None),
        _ => Err(invalid()),
    }
}

fn invalid() -> Fault {
    Fault::invalid("model authorization data is invalid")
}
