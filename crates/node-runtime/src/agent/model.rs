use super::*;
mod authorization;
mod cloud;
mod diagnostics;
mod errors;
pub(super) mod history;
mod hosted;
mod opencode;
mod reasoning;
mod retry;
pub(super) mod timing;
use adk_core::Llm;
use adk_model::{
    anthropic::{AnthropicClient, AnthropicConfig},
    deepseek::{DeepSeekClient, DeepSeekConfig},
    gemini::GeminiModel,
    openai::{
        OpenAIResponsesClient, OpenAIResponsesConfig, ReasoningSummary, RequestAdapter,
        ResponseObserver,
    },
    openai_compatible::{OpenAICompatible, OpenAICompatibleConfig, ReasoningReplayField},
    retry::RetryConfig,
};
pub(super) use diagnostics::cause;
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
            api: opencode::api(provider.api, &config.model)?,
            ingress: ingress.clone(),
            turn: invocation.turn.id,
            session: invocation.turn.session,
            stop: stop.clone(),
        }),
        ingress: ingress.clone(),
        turn: invocation.turn.id,
        current: invocation.message.attachments.clone(),
        vision: metadata.is_none_or(|model| model.vision),
        api: attachments::api(provider, opencode::api(provider.api, &config.model)?),
        tools: metadata.is_none_or(|model| model.tools),
        stop: stop.clone(),
        identity: crate::logging::Context {
            node: Some(ingress.node),
            session: Some(invocation.turn.session),
            turn: Some(invocation.turn.id),
            provider: Some(provider.id),
            provider_name: Some(provider.name.clone()),
            model: Some(config.model.clone()),
            ..Default::default()
        },
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
    let effort = if metadata.is_none_or(|model| {
        model
            .capabilities
            .as_ref()
            .and_then(|value| value.reasoning)
            != Some(false)
    }) {
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
        let model = if matches!(provider.api, ModelApi::OpenCodeGo | ModelApi::OpenCodeZen) {
            opencode::build(provider, config, key, effort, session)?
        } else {
            native(
                provider,
                config,
                key,
                effort,
                hosted::session_adapter(provider, &config.model, session),
                None,
            )?
        };
        errors::wrap(model, credential.into_iter().collect())
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
    observer: Option<ResponseObserver>,
) -> Result<Arc<dyn Llm>, Fault> {
    let inner: Arc<dyn Llm> = match provider.api {
        ModelApi::OpenCodeGo | ModelApi::OpenCodeZen => {
            return Err(Fault::invalid("OpenCode requires a conversation identity"));
        }
        ModelApi::AzureOpenAi | ModelApi::AzureAi => {
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
            if provider.authentication == Authentication::ApiKey {
                client = client
                    .with_default_headers(hosted::headers())
                    .map_err(error)?;
                if hosted::replay(provider, &config.model) {
                    client =
                        client.with_reasoning_replay_field(ReasoningReplayField::ReasoningContent);
                }
            }
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
                && provider.models.iter().any(|model| {
                    model.id == config.model
                        && (model.reasoning
                            || (effort != Effort::Default
                                && model
                                    .capabilities
                                    .as_ref()
                                    .and_then(|value| value.reasoning)
                                    != Some(false)))
                }) {
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
            if provider.authentication == Authentication::ApiKey {
                client = client
                    .with_default_headers(hosted::headers())
                    .map_err(error)?;
            }
            if let Some(adapter) = adapter {
                client = client.with_request_adapter(adapter).map_err(error)?;
            }
            if let Some(observer) = observer {
                client = client.with_response_observer(observer).map_err(error)?;
            }
            Arc::new(client)
        }
        ModelApi::Anthropic => {
            let endpoint = crate::providers::anthropic_base_url(&provider.endpoint)?;
            let native = AnthropicConfig::new(key, &config.model).with_base_url(endpoint.as_str());
            let native = reasoning::anthropic(native, effort)?;
            let mut client = AnthropicClient::new(native)
                .map_err(error)?
                .with_retry_config(RetryConfig::disabled());
            if provider.authentication == Authentication::ApiKey {
                client = client.with_default_headers(hosted::headers());
            }
            if let Some(adapter) = adapter {
                client = client.with_request_adapter(adapter);
            }
            Arc::new(client)
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
    Fault::new(ErrorCode::Unavailable, summary(&error))
}

fn summary(error: &adk_core::AdkError) -> String {
    // Unchecked provider bodies never enter public history.
    let mut message = format!("Agent execution failed ({})", error.code);
    if let Some(status) = error.details.upstream_status_code {
        // Keep classification separate from the optional redacted diagnostic body.
        message.push_str(&format!("; HTTP {status}; {}", error.category));
    }
    message
}

#[cfg(test)]
mod error_tests {
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

    #[test]
    fn preserves_shared_usage_codes() {
        for code in [
            "model.openai_responses.subscription_sharing_usage_limit_exceeded",
            "model.openai_responses.subscription_sharing_usage_unavailable",
        ] {
            let failure = AdkError::new(
                ErrorComponent::Model,
                ErrorCategory::Forbidden,
                code,
                "private echoed credential",
            );
            assert_eq!(
                error(failure).message,
                format!("Agent execution failed ({code})")
            );
        }
    }
}
