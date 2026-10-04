use super::*;
mod authorization;
mod cloud;
pub(super) mod history;
mod opencode;
mod reasoning;
mod retry;
pub(super) mod timing;
use adk_core::Llm;
use adk_model::{
    anthropic::{AnthropicClient, AnthropicConfig},
    deepseek::{DeepSeekClient, DeepSeekConfig},
    gemini::GeminiModel,
    openai::{OpenAIResponsesClient, OpenAIResponsesConfig, ReasoningSummary, RequestAdapter},
    openai_compatible::{OpenAICompatible, OpenAICompatibleConfig},
    retry::RetryConfig,
};
use sailry_protocol::{
    Authentication, Effort, SessionConfig,
    conversation::{ModelApi, Provider},
};

pub(super) async fn build(
    ingress: &Arc<Ingress>,
    invocation: &Invocation,
    stop: &CancellationToken,
) -> Result<Arc<dyn Llm>, Fault> {
    let provider = invocation.provider.as_ref().ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "provider was not configured when this turn was queued",
        )
    })?;
    let config = &invocation.turn.config;
    let inner = configured(
        ingress,
        provider,
        config,
        invocation.turn.session,
        invocation.child.is_some(),
        stop,
    )
    .await?;
    let metadata = provider
        .models
        .iter()
        .find(|model| model.id == config.model);
    Ok(Arc::new(attachments::Model {
        inner: Arc::new(retry::Model {
            inner,
            ingress: ingress.clone(),
            turn: invocation.turn.id,
            session: invocation.turn.session,
            stop: stop.clone(),
        }),
        ingress: ingress.clone(),
        turn: invocation.turn.id,
        vision: metadata.is_some_and(|model| model.vision),
        api: matches!(
            provider.authentication,
            Authentication::ApiKey | Authentication::Host
        )
        .then_some(opencode::api(provider.api, &config.model)?),
        tools: metadata.is_some_and(|model| model.tools),
        stop: stop.clone(),
    }))
}

pub(super) async fn configured(
    ingress: &Arc<Ingress>,
    provider: &Provider,
    config: &SessionConfig,
    session: sailry_protocol::SessionId,
    delegated: bool,
    stop: &CancellationToken,
) -> Result<Arc<dyn Llm>, Fault> {
    if !provider.enabled
        || (!provider.models.is_empty()
            && !provider.models.iter().any(|model| model.id == config.model))
    {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "provider or model was unavailable when this turn was sent",
        ));
    }
    let api = opencode::api(provider.api, &config.model)?;
    let metadata = provider
        .models
        .iter()
        .find(|model| model.id == config.model);
    if let Some(model) = metadata {
        model.validate_effort(api, config.effort)?;
    }
    let effort = if metadata.is_none_or(|model| model.reasoning) {
        config.effort
    } else {
        Effort::Default
    };
    effort.validate(api, metadata.map_or(0, |model| model.output))?;
    let inner = if provider.authentication == Authentication::ApiKey {
        let credential = match &config.credential {
            Some(reference) => Some(
                ingress
                    .resolve_credential(reference.clone(), config.provider)
                    .await?,
            ),
            None => None,
        };
        // Explicit local endpoints may be credential-free; never read controller defaults.
        let key = credential.as_ref().map_or("", |secret| secret.expose());
        if matches!(provider.api, ModelApi::OpenCodeGo | ModelApi::OpenCodeZen) {
            opencode::build(provider, config, key, effort, session)?
        } else {
            native(provider, config, key, effort, None)?
        }
    } else if provider.authentication == Authentication::Host {
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "cloud initialization cancelled")),
            result = tokio::time::timeout(std::time::Duration::from_secs(30), cloud::host(provider, &config.model, effort)) =>
                result.map_err(|_| Fault::new(ErrorCode::Unavailable, "cloud identity initialization timed out"))??,
        }
    } else {
        crate::providers::login::validate(provider)?;
        Arc::new(authorization::Model {
            ingress: ingress.clone(),
            provider: provider.clone(),
            config: config.clone(),
            session,
            delegated,
            effort,
            stop: stop.clone(),
        }) as Arc<dyn Llm>
    };
    Ok(inner)
}

pub(super) fn native(
    provider: &Provider,
    config: &SessionConfig,
    key: &str,
    effort: Effort,
    adapter: Option<RequestAdapter>,
) -> Result<Arc<dyn Llm>, Fault> {
    let inner: Arc<dyn Llm> = match provider.api {
        ModelApi::OpenCodeGo | ModelApi::OpenCodeZen => {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "OpenCode requires a conversation identity",
            ));
        }
        ModelApi::AzureOpenAi | ModelApi::AzureAi | ModelApi::Bedrock | ModelApi::Vertex => {
            cloud::keyed(provider, &config.model, key, effort)?
        }
        ModelApi::DeepSeek => Arc::new(
            DeepSeekClient::new(reasoning::deepseek(
                DeepSeekConfig::new(key, &config.model).with_base_url(&provider.endpoint),
                effort,
            )?)
            .map_err(error)?
            .with_retry_config(RetryConfig::disabled()),
        ),
        ModelApi::ChatCompletions => {
            let mut client = OpenAICompatible::new_with_reasoning_effort(
                OpenAICompatibleConfig::new(key, &config.model).with_base_url(&provider.endpoint),
                reasoning::openai(effort)?,
            )
            .map_err(error)?
            .with_retry_config(RetryConfig::disabled());
            if let Some(adapter) = adapter {
                client = client.with_request_adapter(adapter);
            }
            Arc::new(client)
        }
        ModelApi::Responses => {
            let config = OpenAIResponsesConfig::new(key, &config.model)
                .with_base_url(&provider.endpoint)
                // Response dialect is determined by the endpoint, not API-key presence.
                .with_open_responses_mode(
                    config.credential.is_none()
                        || provider.endpoint.trim_end_matches('/') != "https://api.openai.com/v1",
                );
            let config = if effort != Effort::Disabled
                && provider
                    .models
                    .iter()
                    .any(|model| model.id == config.model && model.reasoning)
            {
                config.with_reasoning_summary(ReasoningSummary::Auto)
            } else {
                config
            };
            let client = match reasoning::openai(effort)? {
                Some(effort) => OpenAIResponsesClient::new_with_reasoning_effort(config, effort),
                None => OpenAIResponsesClient::new(config),
            };
            let mut client = client
                .map_err(error)?
                .with_retry_config(RetryConfig::disabled());
            if let Some(adapter) = adapter {
                client = client.with_request_adapter(adapter).map_err(error)?;
            }
            Arc::new(client)
        }
        ModelApi::Anthropic => {
            let endpoint = crate::providers::anthropic_base_url(&provider.endpoint)?;
            let native = AnthropicConfig::new(key, &config.model).with_base_url(endpoint.as_str());
            let native = reasoning::anthropic(native, effort)?;
            Arc::new(
                AnthropicClient::new(native)
                    .map_err(error)?
                    .with_retry_config(RetryConfig::disabled()),
            )
        }
        ModelApi::Gemini => {
            let endpoint = format!("{}/", provider.endpoint.trim_end_matches('/'));
            let mut native = GeminiModel::new_with_base_url(key, &config.model, endpoint)
                .map_err(error)?
                .with_retry_config(RetryConfig::disabled());
            if let Some(thinking) = reasoning::gemini(effort)? {
                native = native.with_thinking_config(thinking);
            }
            Arc::new(native)
        }
    };
    Ok(inner)
}

pub(super) fn error(error: adk_core::AdkError) -> Fault {
    if let Some(fault) =
        std::error::Error::source(&error).and_then(|source| source.downcast_ref::<Fault>())
    {
        return fault.clone();
    }
    // Provider response bodies may echo authentication values. Public history
    // carries ADK's static diagnostic code, never an unchecked upstream body.
    let mut message = format!("Agent execution failed ({})", error.code);
    if let Some(status) = error.details.upstream_status_code {
        // Only structured status/category are safe to publish; never response
        // text, provider metadata, request IDs or credential-bearing sources.
        message.push_str(&format!("; HTTP {status}; {}", error.category));
    }
    Fault::new(ErrorCode::Unavailable, message)
}

#[cfg(test)]
mod errors {
    use super::*;
    use adk_core::{AdkError, ErrorCategory, ErrorComponent};

    #[test]
    fn retains_status_without_provider_content() {
        for (status, category) in [
            (400, ErrorCategory::InvalidInput),
            (401, ErrorCategory::Unauthorized),
            (429, ErrorCategory::RateLimited),
        ] {
            let mut failure = AdkError::new(
                ErrorComponent::Model,
                category,
                "model.openai_compat.api_error",
                "private echoed credential",
            )
            .with_upstream_status(status)
            .with_provider("private provider");
            failure.details.request_id = Some("private request".into());
            failure
                .details
                .metadata
                .insert("body".into(), serde_json::json!("private metadata"));
            let fault = error(failure);
            assert_eq!(
                fault.message,
                format!(
                    "Agent execution failed (model.openai_compat.api_error); HTTP {status}; {category}"
                )
            );
            assert!(!fault.message.contains("private"));
        }
    }
}
