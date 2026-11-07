//! Context discovery policy derived from Goose 14741e9a providers/openai.rs.
//! Apache-2.0; see third_party_licenses/goose-Apache-2.0.txt.
use super::*;
use crate::providers::request as policy;
use serde_json::Value;
use std::time::Duration;

pub(in crate::agent) async fn limit(
    ingress: &Arc<Ingress>,
    invocation: &Invocation,
    stop: &CancellationToken,
) -> usize {
    let Some(provider) = &invocation.provider else {
        return 128_000;
    };
    let config = &invocation.turn.config;
    if let Some(limit) = provider
        .models
        .iter()
        .find(|model| model.id == config.model)
        .and_then(|model| (model.context > 0).then_some(model.context as usize))
    {
        return limit;
    }
    let fallback = gdk::context(provider.api, &config.model);
    if !matches!(
        provider.api,
        ModelApi::Responses
            | ModelApi::ChatCompletions
            | ModelApi::DeepSeek
            | ModelApi::OpenCodeGo
            | ModelApi::OpenCodeZen
            | ModelApi::AzureAi
    ) {
        return fallback;
    }
    let resolved = match policy::current(ingress, provider, config.credential.as_ref(), stop).await
    {
        Ok(resolved) => resolved,
        Err(_) => return fallback,
    };
    let Ok(fingerprint) = registry::fingerprint(&resolved.provider, config, resolved.revision)
    else {
        return fallback;
    };
    if let Some(limit) = ingress
        .model_clients
        .context(invocation.turn.session, &fingerprint)
    {
        return limit.unwrap_or(fallback);
    }
    let result = tokio::select! {
        biased;
        _ = stop.cancelled() => return fallback,
        result = tokio::time::timeout(Duration::from_secs(5), discover(ingress, &resolved, &config.model)) => result,
    };
    let failed = !matches!(result, Ok(Ok(_)));
    let limit = result
        .ok()
        .and_then(Result::ok)
        .flatten()
        .filter(|limit| *limit > 0);
    ingress
        .model_clients
        .remember(invocation.turn.session, fingerprint, limit, failed);
    limit.unwrap_or(fallback)
}

async fn discover(
    ingress: &Ingress,
    resolved: &policy::Resolved,
    model: &str,
) -> Result<Option<usize>, Fault> {
    let provider = &resolved.provider;
    let headers = policy::headers(provider, &resolved.credentials)?;
    let mut endpoint = crate::providers::endpoint_url(&provider.endpoint)?;
    let client = ingress.model_clients.http(Some(endpoint.origin()))?;
    if provider.api == ModelApi::AzureAi {
        return rig::context(&client, provider, model, headers)
            .await
            .map(Some)
            .map_err(errors::fault);
    }
    let path = endpoint.path().trim_end_matches('/');
    let path = if let Some(path) = path
        .strip_suffix("/chat/completions")
        .or_else(|| path.strip_suffix("/responses"))
    {
        format!("{path}/models")
    } else if matches!(provider.api, ModelApi::OpenCodeGo | ModelApi::OpenCodeZen)
        && !path.rsplit('/').next().is_some_and(|segment| {
            segment.strip_prefix('v').is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
    {
        format!("{path}/v1/models")
    } else {
        format!("{path}/models")
    };
    endpoint.set_path(&path);
    let response = client
        .get(endpoint)
        .headers(headers)
        .send()
        .await
        .map_err(|_| Fault::new(ErrorCode::Unavailable, "model context discovery failed"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "model context discovery failed",
        ));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|_| Fault::invalid("model context response is invalid"))?;
    Ok(allocated(&body, model))
}

fn allocated(body: &Value, model: &str) -> Option<usize> {
    let models = body.get("data")?.as_array()?;
    let selected = models
        .iter()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some(model))
        .or_else(|| match models.as_slice() {
            [only] => Some(only),
            _ => None,
        })?;
    usize::try_from(selected.pointer("/meta/n_ctx")?.as_u64()?)
        .ok()
        .filter(|limit| *limit > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn selects_allocated_context() {
        let body = json!({"data":[{"id":"loaded","meta":{"n_ctx":32768}}]});
        assert_eq!(allocated(&body, "alias"), Some(32768));
        let body = json!({"data":[{"id":"first","meta":{"n_ctx":16384}},{"id":"second","meta":{"n_ctx":32768}}]});
        assert_eq!(allocated(&body, "first"), Some(16384));
        assert_eq!(allocated(&body, "missing"), None);
        assert_eq!(
            allocated(&json!({"data":[{"id":"zero","meta":{"n_ctx":0}}]}), "zero"),
            None
        );
    }
}
