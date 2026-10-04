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

pub(crate) struct Input {
    url: Url,
    method: Method,
    headers: HeaderMap,
    body: Option<String>,
    timeout: Duration,
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}
fn unknown(message: &str) -> Fault {
    Fault::new(ErrorCode::OutcomeUnknown, message)
}

fn url(value: &str) -> Result<Url, Fault> {
    let url = Url::parse(value).map_err(|_| invalid("invalid plugin HTTP URL"))?;
    if value.len() > 4096
        || !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            "plugin HTTP URL must use HTTP(S) without credentials or a fragment",
        ));
    }
    Ok(url)
}

fn header(value: &str) -> Result<HeaderName, Fault> {
    let header = HeaderName::from_bytes(value.as_bytes())
        .map_err(|_| invalid("invalid plugin HTTP header"))?;
    if matches!(
        header.as_str(),
        "host"
            | "connection"
            | "content-length"
            | "transfer-encoding"
            | "proxy-authorization"
            | "upgrade"
    ) {
        return Err(invalid("plugin HTTP header is controlled by the transport"));
    }
    Ok(header)
}

pub(crate) fn binding(binding: &Binding) -> Result<(Url, HeaderName, &str), Fault> {
    if binding.unsupported {
        return Err(invalid("unsupported protected setting binding"));
    }
    if binding.server.is_some() || binding.env.is_some() {
        return Err(invalid("HTTP credentials cannot target an MCP slot"));
    }
    let origin = url(binding
        .origin
        .as_deref()
        .ok_or_else(|| invalid("HTTP credential origin is required"))?)?;
    if origin.path() != "/" || origin.query().is_some() {
        return Err(invalid(
            "HTTP credential must declare an origin without a path or query",
        ));
    }
    let header = header(
        binding
            .header
            .as_deref()
            .ok_or_else(|| invalid("HTTP credential header is required"))?,
    )?;
    let prefix = binding.prefix.as_deref().unwrap_or("");
    if prefix.len() > 128 || HeaderValue::from_str(prefix).is_err() {
        return Err(invalid("invalid HTTP credential prefix"));
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
        || request.headers.len() > http::MAX_HEADERS
        || !matches!(
            request.method.as_str(),
            "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS"
        )
    {
        return Err(invalid("invalid plugin HTTP method, timeout or size"));
    }
    let method = Method::from_bytes(request.method.as_bytes())
        .map_err(|_| invalid("invalid HTTP method"))?;
    let mut headers = HeaderMap::new();
    for (name, value) in &request.headers {
        if value.len() > 8192 {
            return Err(invalid("plugin HTTP header exceeds the size limit"));
        }
        let value = HeaderValue::from_str(value)
            .map_err(|_| invalid("invalid plugin HTTP header value"))?;
        if headers.insert(header(name)?, value).is_some() {
            return Err(invalid("duplicate plugin HTTP header"));
        }
    }
    if let Some(field) = &request.credential {
        let binding = schema
            .and_then(|schema| schema.properties.get(field))
            .and_then(|field| field.secret.as_ref())
            .ok_or_else(|| invalid("HTTP credential is not declared"))?;
        let (origin, name, prefix) = self::binding(binding)?;
        if url.origin() != origin.origin() {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "HTTP credential belongs to another origin",
            ));
        }
        if headers.contains_key(&name) {
            return Err(invalid("HTTP credential header cannot be overridden"));
        }
        let secret = settings
            .and_then(|settings| settings.secrets.get(field))
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::NotConfigured,
                    "HTTP credential is not configured",
                )
            })?;
        let mut value = HeaderValue::from_str(&format!("{prefix}{}", secret.expose()))
            .map_err(|_| invalid("HTTP credential is not a valid header value"))?;
        value.set_sensitive(true);
        headers.insert(name, value);
    }
    Ok(Input {
        url,
        method,
        headers,
        body: request.body.clone(),
        timeout: Duration::from_millis(request.timeout_ms.into()),
    })
}

pub(crate) async fn run(input: Input, stop: CancellationToken) -> Result<http::Response, Fault> {
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
        if response.headers().len() > http::MAX_HEADERS
            || response
                .content_length()
                .is_some_and(|length| length > http::MAX_BODY as u64)
        {
            return Err(unknown("plugin HTTP response exceeds the size limit"));
        }
        for (name, value) in response.headers() {
            let value = value
                .to_str()
                .map_err(|_| unknown("plugin HTTP response has a non-text header"))?;
            if value.len() > 8192 {
                return Err(unknown(
                    "plugin HTTP response header exceeds the size limit",
                ));
            }
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
        Ok(http::Response {
            status,
            headers,
            body,
        })
    };
    tokio::select! {
        biased;
        _ = stop.cancelled() => Err(unknown("plugin HTTP request was interrupted")),
        result = operation => result,
    }
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
}
