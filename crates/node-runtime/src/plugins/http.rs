//! Bounded requests use Node networking and protected credential bindings.
use reqwest::{
    Method, Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    plugin::{
        http,
        settings::{Binding, Schema},
    },
};
use std::{collections::BTreeMap, time::Duration};
pub(crate) mod provider;
pub(crate) use provider::prepare as prepare_provider;

#[derive(Clone)]
pub(crate) struct Input {
    url: Url,
    method: Method,
    headers: HeaderMap,
    body: Option<String>,
    timeout: Duration,
    pub(crate) connector: Option<(
        super::connectors::Target,
        sailry_protocol::plugin::connector::Injection,
        bool,
    )>,
    provider: Option<Box<sailry_protocol::conversation::Provider>>,
    cache: Option<provider::cache::Policy>,
}

fn unknown(message: &str) -> Fault {
    Fault::new(ErrorCode::OutcomeUnknown, message)
}

fn url(value: &str) -> Result<Url, Fault> {
    let url = Url::parse(value)
        .map_err(|_| sailry_protocol::Fault::invalid("invalid plugin HTTP URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(sailry_protocol::Fault::invalid(
            "plugin HTTP URL must use HTTP(S) without credentials or a fragment",
        ));
    }
    Ok(url)
}

fn header(value: &str) -> Result<HeaderName, Fault> {
    let header = HeaderName::from_bytes(value.as_bytes())
        .map_err(|_| sailry_protocol::Fault::invalid("invalid plugin HTTP header"))?;
    if matches!(
        header.as_str(),
        "host"
            | "connection"
            | "content-length"
            | "transfer-encoding"
            | "proxy-authorization"
            | "upgrade"
    ) {
        return Err(sailry_protocol::Fault::invalid(
            "plugin HTTP header is controlled by the transport",
        ));
    }
    Ok(header)
}

fn insert(
    headers: &mut HeaderMap,
    name: HeaderName,
    value: HeaderValue,
) -> Result<Option<HeaderValue>, Fault> {
    headers
        .try_insert(name, value)
        .map_err(|_| Fault::invalid("plugin HTTP headers exceed the HTTP implementation capacity"))
}

pub(crate) fn binding(binding: &Binding) -> Result<(Url, HeaderName, &str), Fault> {
    if binding.unsupported {
        return Err(sailry_protocol::Fault::invalid(
            "unsupported protected setting binding",
        ));
    }
    if binding.server.is_some() || binding.env.is_some() {
        return Err(sailry_protocol::Fault::invalid(
            "HTTP credentials cannot target an MCP slot",
        ));
    }
    let origin = url(binding
        .origin
        .as_deref()
        .ok_or_else(|| sailry_protocol::Fault::invalid("HTTP credential origin is required"))?)?;
    if origin.path() != "/" || origin.query().is_some() {
        return Err(sailry_protocol::Fault::invalid(
            "HTTP credential must declare an origin without a path or query",
        ));
    }
    let header =
        header(binding.header.as_deref().ok_or_else(|| {
            sailry_protocol::Fault::invalid("HTTP credential header is required")
        })?)?;
    let prefix = binding.prefix.as_deref().unwrap_or("");
    if HeaderValue::from_str(prefix).is_err() {
        return Err(sailry_protocol::Fault::invalid(
            "invalid HTTP credential prefix",
        ));
    }
    Ok((origin, header, prefix))
}

pub(crate) fn prepare(
    request: &http::Request,
    schema: Option<&Schema>,
    settings: Option<&super::settings::Resolved>,
) -> Result<Input, Fault> {
    let url = url(&request.url)?;
    if !(1..=http::MAX_TIMEOUT_MS).contains(&request.timeout_ms)
        || request
            .body
            .as_ref()
            .is_some_and(|body| body.len() > http::MAX_BODY)
        || !matches!(
            request.method.as_str(),
            "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS"
        )
    {
        return Err(sailry_protocol::Fault::invalid(
            "invalid plugin HTTP method, timeout or size",
        ));
    }
    let method = Method::from_bytes(request.method.as_bytes())
        .map_err(|_| sailry_protocol::Fault::invalid("invalid HTTP method"))?;
    let mut headers = HeaderMap::new();
    for (name, value) in &request.headers {
        let value = HeaderValue::from_str(value)
            .map_err(|_| sailry_protocol::Fault::invalid("invalid plugin HTTP header value"))?;
        if insert(&mut headers, header(name)?, value)?.is_some() {
            return Err(sailry_protocol::Fault::invalid(
                "duplicate plugin HTTP header",
            ));
        }
    }
    if let Some(field) = &request.credential {
        let binding = schema
            .and_then(|schema| schema.properties.get(field))
            .and_then(|field| field.secret.as_ref())
            .ok_or_else(|| sailry_protocol::Fault::invalid("HTTP credential is not declared"))?;
        let (origin, name, prefix) = self::binding(binding)?;
        if url.origin() != origin.origin() {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "HTTP credential belongs to another origin",
            ));
        }
        if headers.contains_key(&name) {
            return Err(sailry_protocol::Fault::invalid(
                "HTTP credential header cannot be overridden",
            ));
        }
        let secret = settings
            .and_then(|settings| settings.secrets.get(field))
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::NotConfigured,
                    "HTTP credential is not configured",
                )
            })?;
        let mut value =
            HeaderValue::from_str(&format!("{prefix}{}", secret.expose())).map_err(|_| {
                sailry_protocol::Fault::invalid("HTTP credential is not a valid header value")
            })?;
        value.set_sensitive(true);
        insert(&mut headers, name, value)?;
    }
    if metadata_size(&url, &headers) > sailry_protocol::MAX_FRAME_BYTES {
        return Err(Fault::invalid(
            "plugin HTTP metadata exceeds the transport budget",
        ));
    }
    Ok(Input {
        url,
        method,
        headers,
        body: request.body.clone(),
        timeout: Duration::from_millis(request.timeout_ms.into()),
        connector: None,
        provider: None,
        cache: None,
    })
}

pub(crate) async fn run(
    mut input: Input,
    ingress: &crate::store::Ingress,
    stop: CancellationToken,
) -> Result<http::Response, Fault> {
    if let Some((target, injection, idempotent)) = input.connector.take() {
        let name = header(
            injection
                .header
                .as_deref()
                .ok_or_else(|| Fault::invalid("connector HTTP header is unavailable"))?,
        )?;
        if input.headers.contains_key(&name) {
            return Err(Fault::invalid("connector header cannot be overridden"));
        }
        let service = &ingress.plugins.connectors;
        let mut secrets = target.secrets.values().cloned().collect::<Vec<_>>();
        let mut rotated: Option<crate::plugins::connectors::Access> = None;
        for attempt in 0..2 {
            let access = if let Some(field) = &injection.field {
                // Field injection does not enter access(), so it keeps its one live fence check.
                let current = ingress
                    .connector_target(target.id, target.revision, target.checking)
                    .await?;
                crate::plugins::connectors::Access {
                    token: target.secrets.get(field).cloned().ok_or_else(|| {
                        Fault::new(ErrorCode::NotConfigured, "connector secret is unavailable")
                    })?,
                    revision: current.grant_revision,
                }
            } else if let Some(token) = rotated.take() {
                token
            } else {
                service.access(&target, ingress, None, &stop).await?
            };
            secrets.push(access.token.clone());
            let mut value =
                HeaderValue::from_str(&format!("{}{}", injection.prefix, access.token.expose()))
                    .map_err(|_| {
                        Fault::invalid("connector credential is not a valid header value")
                    })?;
            value.set_sensitive(true);
            insert(&mut input.headers, name.clone(), value)?;
            input.url = Url::parse(&service.resource(input.url.as_str()))
                .map_err(|_| Fault::invalid("invalid connector HTTP URL"))?;
            let response = send(input.clone(), Some(secrets.clone()), stop.clone()).await?;
            if response.status != 401 {
                service.accepted(target.id, access.revision)?;
                return Ok(response);
            }
            use crate::plugins::connectors::Rejection;
            let rejected = service
                .reject(&target, ingress, access.revision, attempt != 0)
                .await?;
            let force = match rejected {
                Rejection::Stop => return Ok(response),
                Rejection::Current => None,
                Rejection::Renew => Some(access.revision),
            };
            if attempt != 0 {
                return Ok(response);
            }
            // Rotate a rejected token even when the side effect cannot be replayed.
            rotated = Some(service.access(&target, ingress, force, &stop).await?);
            if !idempotent {
                return Ok(response);
            }
        }
        unreachable!()
    }
    let authorization = provider::authorize(&mut input, ingress, &stop).await?;
    if let (Some(policy), Some(authorization)) = (input.cache.take(), authorization.as_ref()) {
        let key = policy.key(&authorization.credential);
        let protection = authorization.secrets.clone();
        return ingress
            .plugins
            .provider_reads
            .fetch(key, policy, &stop, || {
                send(input, Some(protection), stop.clone())
            })
            .await;
    }
    send(
        input,
        authorization.map(|authorization| authorization.secrets),
        stop,
    )
    .await
}

async fn send(
    input: Input,
    protection: Option<Vec<sailry_protocol::Secret>>,
    stop: CancellationToken,
) -> Result<http::Response, Fault> {
    if metadata_size(&input.url, &input.headers) > sailry_protocol::MAX_FRAME_BYTES {
        return Err(Fault::invalid(
            "plugin HTTP metadata exceeds the transport budget",
        ));
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(input.timeout)
        .build()
        .map_err(|_| Fault::new(ErrorCode::Unavailable, "plugin HTTP client is unavailable"))?;
    let operation = async move {
        let mut request = client
            .request(input.method, input.url)
            .headers(input.headers);
        if let Some(body) = input.body {
            request = request.body(body);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| unknown("plugin HTTP result is unknown"))?;
        let status = response.status().as_u16();
        let mut headers = BTreeMap::new();
        if response
            .content_length()
            .is_some_and(|length| length > http::MAX_BODY as u64)
        {
            return Err(unknown("plugin HTTP response exceeds the size limit"));
        }
        if metadata_size(response.url(), response.headers()) > sailry_protocol::MAX_FRAME_BYTES {
            return Err(unknown(
                "plugin HTTP response metadata exceeds the size budget",
            ));
        }
        for (name, value) in response.headers() {
            let value = value
                .to_str()
                .map_err(|_| unknown("plugin HTTP response has a non-text header"))?;
            headers
                .entry(name.to_string())
                .or_insert_with(Vec::new)
                .push(value.to_owned());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| unknown("plugin HTTP response was interrupted"))?
        {
            if body.len().saturating_add(chunk.len()) > http::MAX_BODY {
                return Err(unknown("plugin HTTP response exceeds the size limit"));
            }
            body.extend_from_slice(&chunk);
        }
        let body =
            String::from_utf8(body).map_err(|_| unknown("plugin HTTP response is not UTF-8"))?;
        let mut result = http::Response {
            status,
            headers,
            body,
        };
        if let Some(secrets) = &protection {
            provider::protect(&mut result, secrets)?;
        }
        if serde_json::to_vec(&result)
            .map_err(|_| unknown("invalid plugin HTTP response"))?
            .len()
            > sailry_protocol::MAX_FRAME_BYTES - 64 * 1024
        {
            return Err(unknown("plugin HTTP response exceeds the transport budget"));
        }
        Ok(result)
    };
    tokio::select! {
        biased;
        _ = stop.cancelled() => Err(unknown("plugin HTTP request was interrupted")),
        result = operation => result,
    }
}

fn metadata_size(url: &Url, headers: &HeaderMap) -> usize {
    headers
        .iter()
        .fold(url.as_str().len(), |size, (name, value)| {
            size.saturating_add(name.as_str().len())
                .saturating_add(value.as_bytes().len())
        })
}

#[cfg(test)]
mod protected_binding {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_unsupported_metadata_before_using_the_target() {
        for extra in [json!({"unused": true}), json!({"unsupported": true})] {
            let mut value = json!({"origin": "https://example.test", "header": "Authorization"});
            value
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let declaration: Binding = serde_json::from_value(value).unwrap();
            let error = binding(&declaration).unwrap_err();
            assert_eq!(error.message, "unsupported protected setting binding");
        }
    }

    #[test]
    fn accepts_long_header_prefixes() {
        let prefix = "x".repeat(129);
        let declaration: Binding = serde_json::from_value(json!({
            "origin":"https://example.test", "header":"Authorization", "prefix":prefix
        }))
        .unwrap();
        assert_eq!(binding(&declaration).unwrap().2, prefix);
    }
}

#[cfg(test)]
mod budgets {
    use super::*;

    #[test]
    fn preserves_larger_metadata_within_transport() {
        let request = http::Request {
            connector: None,
            connection: None,
            idempotent: false,
            url: format!("https://example.test/?q={}", "x".repeat(4097)),
            method: "GET".into(),
            headers: (0..33)
                .map(|index| (format!("x-fixture-{index}"), "x".repeat(8193)))
                .collect(),
            body: None,
            credential: None,
            timeout_ms: 1000,
        };
        let prepared = prepare(&request, None, None).unwrap();
        assert_eq!(prepared.url.as_str(), request.url);
        assert_eq!(prepared.headers.len(), request.headers.len());
        for (name, value) in &request.headers {
            assert_eq!(
                prepared
                    .headers
                    .get(name.as_str())
                    .unwrap()
                    .to_str()
                    .unwrap(),
                value
            );
        }
        let mut oversized = request.clone();
        oversized.headers.insert(
            "x-large".into(),
            "x".repeat(sailry_protocol::MAX_FRAME_BYTES),
        );
        assert_eq!(
            prepare(&oversized, None, None).err().unwrap().code,
            ErrorCode::InvalidRequest
        );
        for value in [
            "https://user:secret@example.test/",
            "https://example.test/#fragment",
            "file:///tmp/file",
        ] {
            oversized.url = value.into();
            assert!(url(&oversized.url).is_err());
        }
        oversized = request;
        oversized.headers = (0..40_000)
            .map(|index| (format!("x-fixture-{index}"), "x".into()))
            .collect();
        assert_eq!(
            prepare(&oversized, None, None).err().unwrap().code,
            ErrorCode::InvalidRequest
        );
    }
}
