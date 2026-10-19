//! Declared provider resources reuse protected HTTP without exposing a model grant.
use super::*;
use sailry_protocol::{Secret, conversation::Provider, plugin::model_auth};

pub(crate) fn prepare(
    endpoint: &model_auth::Endpoint,
    fields: &BTreeMap<String, Secret>,
    provider: Provider,
) -> Result<Input, Fault> {
    if !endpoint.valid() || (endpoint.url.contains('{') || endpoint.url.contains('}')) {
        return Err(Fault::invalid("provider resource endpoint is invalid"));
    }
    for (name, value) in &endpoint.headers {
        header(name)?;
        if provider.authentication != sailry_protocol::Authentication::Plugin
            && matches!(
                name.to_ascii_lowercase().as_str(),
                "authorization" | "chatgpt-account-id"
            )
        {
            return Err(Fault::invalid(
                "provider credential header cannot be overridden",
            ));
        }
        if value.contains('{') || value.contains('}') {
            return Err(Fault::invalid("provider resource headers must be fixed"));
        }
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| Fault::new(ErrorCode::Unavailable, "plugin HTTP client is unavailable"))?;
    let request = crate::providers::login::plugin::prepare(&client, endpoint, fields)?
        .build()
        .map_err(|_| Fault::invalid("provider resource request is invalid"))?;
    let body = request
        .body()
        .map(|body| {
            body.as_bytes()
                .filter(|body| body.len() <= http::MAX_BODY)
                .and_then(|body| std::str::from_utf8(body).ok())
                .map(str::to_owned)
                .ok_or_else(|| Fault::invalid("provider resource body exceeds the HTTP budget"))
        })
        .transpose()?;
    if metadata_size(request.url(), request.headers()) > sailry_protocol::MAX_FRAME_BYTES {
        return Err(Fault::invalid(
            "provider resource metadata exceeds the HTTP budget",
        ));
    }
    Ok(Input {
        url: request.url().clone(),
        method: request.method().clone(),
        headers: request.headers().clone(),
        body,
        timeout: Duration::from_secs(30),
        oauth: None,
        application: None,
        provider: Some(Box::new(provider)),
    })
}

pub(super) async fn authorize(
    input: &mut Input,
    ingress: &crate::store::Ingress,
    stop: &CancellationToken,
) -> Result<Option<Vec<Secret>>, Fault> {
    let Some(provider) = input.provider.take() else {
        return Ok(None);
    };
    let reference = provider
        .credential
        .clone()
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "provider sign-in is required"))?;
    let options = crate::providers::login::effective(&provider)?;
    let grant = ingress
        .authorization(
            reference,
            provider.id,
            provider.authentication,
            &options,
            stop,
        )
        .await?
        .grant;
    let headers = if provider.authentication == sailry_protocol::Authentication::Plugin {
        grant.headers(&options)?
    } else {
        grant.resource_headers(input.url.as_str())?
    };
    let secrets = if provider.authentication == sailry_protocol::Authentication::Plugin {
        grant.secrets(&headers)
    } else {
        // Account identity may legitimately occur in a usage response. Actual token
        // values remain protected even when they happen to equal that metadata.
        let mut protected = headers.clone();
        protected.remove("chatgpt-account-id");
        grant.secrets(&protected)
    };
    for (name, value) in &headers {
        if input.headers.contains_key(name) {
            return Err(Fault::invalid(
                "provider credential header cannot be overridden",
            ));
        }
        insert(&mut input.headers, name.clone(), value.clone())?;
    }
    // Reuse the existing native authorization fixture's private routing seam.
    #[cfg(any(test, feature = "test-support"))]
    if let Some(endpoint) = &ingress.authorization_endpoint {
        let mut routed = Url::parse(endpoint)
            .map_err(|_| Fault::invalid("provider resource fixture URL is invalid"))?;
        routed.set_path(input.url.path());
        routed.set_query(input.url.query());
        input.url = routed;
    }
    Ok(Some(secrets))
}

pub(super) fn protect(response: &mut http::Response, secrets: &[Secret]) -> Result<(), Fault> {
    response.headers.retain(|name, _| {
        matches!(
            name.as_str(),
            "content-type" | "retry-after" | "x-request-id" | "request-id"
        )
    });
    if secrets
        .iter()
        .filter(|secret| !secret.expose().is_empty())
        .any(|secret| {
            response.body.contains(secret.expose())
                || response
                    .headers
                    .values()
                    .flatten()
                    .any(|value| value.contains(secret.expose()))
        })
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "provider response exposed a private credential",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_caller_controlled_transport_headers() {
        let provider:Provider=serde_json::from_value(json!({"id":sailry_protocol::ProviderId::new(),"revision":0,"name":"Example","api":"chat_completions","authentication":"api_key","endpoint":"https://example.invalid","enabled":true,"models":[],"default_model":"","credential":null})).unwrap();
        for (name, value) in [
            ("Host", "example.invalid"),
            ("Connection", "close"),
            ("Content-Length", "1"),
            ("X-Input", "{input.header}"),
        ] {
            let endpoint: model_auth::Endpoint = serde_json::from_value(
                json!({"url":"https://example.invalid/status","headers":{name:value}}),
            )
            .unwrap();
            let error = prepare(&endpoint, &BTreeMap::new(), provider.clone())
                .err()
                .expect("invalid header must fail before HTTP");
            assert_eq!(error.code, ErrorCode::InvalidRequest);
        }
    }
}
