use super::*;
use reqwest::header::HeaderMap;
use sailry_protocol::SessionId;
use serde_json::json;

pub(super) fn config(provider: &Provider, session: &SessionConfig, owner: SessionId) -> Config {
    let metadata = provider
        .models
        .iter()
        .find(|model| model.id == session.model);
    let effort = if session.effort == Effort::Default {
        metadata.map_or(Effort::Default, |model| model.default_effort)
    } else {
        session.effort
    };
    let mut config = Config {
        model_name: session.model.clone(),
        context_limit: metadata
            .and_then(|model| (model.context > 0).then_some(model.context as usize)),
        max_tokens: metadata
            .and_then(|model| (model.output > 0).then_some(u64::from(model.output))),
        thinking_effort: effort,
        reasoning: metadata.and_then(|model| {
            model
                .capabilities
                .as_ref()
                .and_then(|capabilities| capabilities.reasoning)
                .or(Some(model.reasoning))
        }),
        supports_vision: Some(metadata.map_or_else(
            || {
                crate::providers::media::supports_api(
                    sailry_protocol::media::Kind::Vision,
                    provider,
                )
            },
            |model| model.vision,
        )),
        ..Config::default()
    };
    if let Effort::Budget(tokens) = effort {
        config.request_params.insert(
            if provider.api == ModelApi::Gemini {
                "thinking_budget"
            } else {
                "budget_tokens"
            }
            .into(),
            json!(tokens),
        );
    }
    if provider.authentication == Authentication::ApiKey
        && matches!(
            provider.api,
            ModelApi::Responses | ModelApi::ChatCompletions
        )
    {
        config
            .request_params
            .insert("prompt_cache_key".into(), json!(owner.to_string()));
    }
    for policy in crate::providers::policy::body(provider, &session.model) {
        config.request_params.extend(policy);
    }
    if metadata
        .and_then(|model| model.capabilities.as_ref())
        .and_then(|capabilities| capabilities.reasoning)
        != Some(false)
    {
        config
            .request_params
            .extend(crate::providers::policy::reasoning(
                provider,
                &session.model,
                effort,
            ));
    }
    if provider.api == ModelApi::DeepSeek && effort == Effort::Disabled {
        config.reasoning = Some(false);
    }
    if matches!(provider.api, ModelApi::Anthropic | ModelApi::AzureAi)
        && effort == Effort::XHigh
        && config.reasoning != Some(false)
        && rig::adaptive(&session.model)
    {
        let output = config
            .request_params
            .entry("output_config")
            .or_insert_with(|| json!({}));
        if let Some(output) = output.as_object_mut() {
            output.insert("effort".into(), json!("xhigh"));
        }
    }
    if provider.api == ModelApi::Responses
        && provider.authentication != Authentication::Copilot
        && metadata.is_some_and(|model| {
            model
                .validate_features(provider.api, &session.features)
                .is_ok()
        })
        && let Some(tier) = session
            .features
            .get("speed")
            .and_then(|value| match value.as_str() {
                "fast" => Some("priority"),
                "ultrafast" => Some("ultrafast"),
                _ => None,
            })
    {
        config
            .request_params
            .insert("service_tier".into(), json!(tier));
    }
    config
}

pub(super) fn native(
    registry: &registry::Registry,
    provider: &Provider,
    model: &str,
    session: SessionId,
    key: &str,
    headers: HeaderMap,
) -> Result<Arc<dyn Model>, Fault> {
    Ok(Arc::new(rig::RigModel::new(
        registry, provider, model, session, key, headers,
    )?))
}

pub(super) fn unavailable(error: impl std::fmt::Display) -> Fault {
    let _ = error;
    Fault::new(
        ErrorCode::Unavailable,
        "model provider could not be configured",
    )
}

#[cfg(test)]
mod tests;
