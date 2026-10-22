use super::*;
mod authorization;
mod cloud;
mod diagnostics;
mod errors;
mod features;
pub(super) mod history;
mod hosted;
mod opencode;
pub(super) mod plugin;
mod reasoning;
pub(crate) mod registry;
mod retry;
pub(super) mod timing;
mod transport;
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
#[cfg(any(test, feature = "test-support"))]
pub(crate) use transport::configure_fixture;

fn timeouts() -> adk_model::HttpTimeouts {
    adk_model::HttpTimeouts::new(
        std::time::Duration::from_secs(30),
        std::time::Duration::from_secs(300),
    )
}

pub(super) async fn build(
    ingress: &Arc<Ingress>,
    invocation: &Invocation,
    stop: &CancellationToken,
    purpose: Purpose,
) -> Result<Arc<dyn Llm>, Fault> {
    let provider = invocation.provider.as_ref().ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "provider was not configured when this turn was queued",
        )
    })?;
    let config = &invocation.turn.config;
    let inner = configure(
        ingress,
        provider,
        config,
        invocation.turn.session,
        invocation.child.is_some(),
        stop,
        purpose,
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
            turn: Some(invocation.turn.id),
            session: invocation.turn.session,
            stop: stop.clone(),
            limit: provider.stream_retries(),
            persistent: matches!(purpose, Purpose::Main),
        }),
        ingress: ingress.clone(),
        turn: invocation.turn.id,
        current: invocation.message.attachments.clone(),
        vision: metadata.is_none_or(|model| model.vision)
            && provider
                .binding
                .as_ref()
                .and_then(|binding| binding.adapter.as_ref())
                .is_none_or(|adapter| adapter.capabilities.vision),
        api: attachments::api(provider, opencode::api(provider.api, &config.model)?),
        tools: metadata.is_none_or(|model| model.tools)
            && provider
                .binding
                .as_ref()
                .and_then(|binding| binding.adapter.as_ref())
                .is_none_or(|adapter| adapter.capabilities.tools),
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
    let inner = configure(
        ingress,
        provider,
        config,
        session,
        delegated,
        stop,
        Purpose::Helper,
    )
    .await?;
    Ok(Arc::new(retry::Model {
        inner,
        ingress: ingress.clone(),
        turn: None,
        session,
        stop: stop.clone(),
        limit: provider.stream_retries(),
        persistent: false,
    }))
}

#[derive(Clone, Copy)]
pub(super) enum Purpose {
    Main,
    Helper,
}

async fn configure(
    ingress: &Arc<Ingress>,
    provider: &Provider,
    config: &SessionConfig,
    session: sailry_protocol::SessionId,
    delegated: bool,
    stop: &CancellationToken,
    purpose: Purpose,
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
        model.validate_features(api, &config.features)?;
    } else if !config.features.is_empty() {
        return Err(Fault::invalid(
            "model feature choices require an advertised model",
        ));
    }
    let resolved = reasoning::resolve(metadata, api, config.effort)?;
    let effort = resolved.effort;
    let inner = if provider.api == ModelApi::Plugin {
        let mut config = config.clone();
        config.effort = effort;
        Arc::new(plugin::Model::new(ingress, provider, &config, session, stop).await?)
            as Arc<dyn Llm>
    } else if provider.authentication == Authentication::ApiKey {
        let credential = match &config.credential {
            Some(reference) => Some(
                ingress
                    .model_credential(reference.clone(), config.provider)
                    .await?,
            ),
            None => None,
        };
        // Explicit local endpoints may be credential-free; never read controller defaults.
        let key = credential
            .as_ref()
            .map_or("", |(_, secret)| secret.expose());
        let websocket = matches!(purpose, Purpose::Main)
            .then(|| {
                ingress
                    .model_clients
                    .capabilities
                    .options(provider, &config.model, session)
            })
            .flatten();
        let fingerprint = registry::fingerprint(
            provider,
            config,
            effort,
            credential.as_ref().map(|(value, _)| value.revision),
            websocket.is_some(),
        )?;
        let entry = ingress.model_clients.get(session, fingerprint, || {
            if matches!(provider.api, ModelApi::OpenCodeGo | ModelApi::OpenCodeZen) {
                opencode::build(provider, config, key, effort, session).map(registry::Built::new)
            } else {
                native(
                    provider,
                    config,
                    key,
                    resolved,
                    hosted::session_adapter(provider, &config.model, session, resolved.encrypted),
                    None,
                    websocket,
                )
            }
        })?;
        if matches!(purpose, Purpose::Main)
            && let Some(client) = entry.responses.clone()
        {
            entry.preconnect(stop, async move {
                let _ = client.preconnect().await;
            });
        }
        let origin = history::origin(
            provider,
            &config.model,
            credential.as_ref().map(|(credential, _)| credential),
            None,
        );
        history::wrap(
            errors::wrap(
                entry.clone(),
                credential.into_iter().map(|(_, secret)| secret).collect(),
            ),
            origin,
        )
    } else {
        crate::providers::login::validate(provider)?;
        let model = Arc::new(authorization::Model {
            ingress: ingress.clone(),
            provider: provider.clone(),
            config: config.clone(),
            session,
            delegated,
            reasoning: resolved,
            purpose,
            stop: stop.clone(),
        });
        if matches!(purpose, Purpose::Main) {
            let _ = model.preconnect().await;
        }
        model as Arc<dyn Llm>
    };
    Ok(if api == ModelApi::Responses {
        features::wrap(inner, config.features.clone())
    } else {
        inner
    })
}

fn request_retry(provider: &Provider) -> adk_model::request_retry::RequestRetryConfig {
    adk_model::request_retry::RequestRetryConfig {
        max_retries: u32::try_from(provider.request_retries()).expect("bounded request retries"),
        ..Default::default()
    }
}

fn native(
    provider: &Provider,
    config: &SessionConfig,
    key: &str,
    reasoning: reasoning::Resolved,
    adapter: Option<RequestAdapter>,
    observer: Option<ResponseObserver>,
    websocket: Option<adk_model::openai::WsOptions>,
) -> Result<registry::Built, Fault> {
    let effort = reasoning.effort;
    let adapter = match provider
        .binding
        .as_ref()
        .and_then(|binding| binding.key_injection.as_ref())
    {
        Some(injection) => {
            use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
            if !matches!(
                provider.api,
                ModelApi::Responses | ModelApi::ChatCompletions | ModelApi::Anthropic
            ) {
                return Err(Fault::invalid(
                    "channel protocol does not support custom key headers",
                ));
            }
            let name = HeaderName::from_bytes(injection.header.as_bytes())
                .map_err(|_| Fault::invalid("channel key header is invalid"))?;
            let mut value = HeaderValue::from_str(&injection.template.replace("{key}", key))
                .map_err(|_| Fault::invalid("channel key header is invalid"))?;
            value.set_sensitive(true);
            Some(Arc::new(
                move |body: &mut serde_json::Value, headers: &mut HeaderMap| {
                    if let Some(adapter) = &adapter {
                        adapter(body, headers)?;
                    }
                    headers.remove("authorization");
                    headers.remove("x-api-key");
                    headers.insert(name.clone(), value.clone());
                    Ok(())
                },
            ) as RequestAdapter)
        }
        None => adapter,
    };
    let key = if provider
        .binding
        .as_ref()
        .is_some_and(|binding| binding.key_injection.is_some())
    {
        ""
    } else {
        key
    };
    let inner: Arc<dyn Llm> = match provider.api {
        ModelApi::Plugin => {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "custom model protocol requires its captured translation",
            ));
        }
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
            .with_retry_config(RetryConfig::disabled())
            .with_request_retry(request_retry(provider))
            .with_timeouts(timeouts())
            .map_err(error)?,
        ),
        ModelApi::ChatCompletions => {
            let mut client = OpenAICompatible::new_with_reasoning_effort(
                OpenAICompatibleConfig::new(key, &config.model).with_base_url(&provider.endpoint),
                reasoning::openai(effort)?,
            )
            .map_err(error)?
            .with_retry_config(RetryConfig::disabled())
            .with_request_retry(request_retry(provider));
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
            Arc::new(client.with_timeouts(timeouts()).map_err(error)?)
        }
        ModelApi::Responses => {
            let config = OpenAIResponsesConfig::new(key, &config.model)
                .with_base_url(&provider.endpoint)
                // Response dialect is determined by the endpoint, not API-key presence.
                .with_open_responses_mode(
                    config.credential.is_none()
                        || provider.endpoint.trim_end_matches('/') != "https://api.openai.com/v1",
                );
            let config = if reasoning.summary {
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
                .with_retry_config(RetryConfig::disabled())
                .with_request_retry(request_retry(provider))
                .with_response_recovery(true);
            if provider.authentication == Authentication::ApiKey {
                client = client
                    .with_default_headers(hosted::headers())
                    .map_err(error)?;
            }
            if let Some(adapter) = adapter {
                client = client.with_request_adapter(adapter);
            }
            if let Some(observer) = observer {
                client = client.with_response_observer(observer);
            }
            let mut client = client.with_timeouts(timeouts()).map_err(error)?;
            if let Some(options) = websocket {
                client = client.with_websocket(options);
            }
            let client = Arc::new(client);
            return Ok(registry::Built {
                inner: client.clone(),
                responses: Some(client),
            });
        }
        ModelApi::Anthropic => {
            let endpoint = crate::providers::anthropic_base_url(&provider.endpoint)?;
            let native = AnthropicConfig::new(key, &config.model).with_base_url(endpoint.as_str());
            let native = reasoning::anthropic(native, effort)?;
            let mut client = AnthropicClient::new(native)
                .map_err(error)?
                .with_retry_config(RetryConfig::disabled())
                .with_request_retry(request_retry(provider));
            if provider.authentication == Authentication::ApiKey {
                client = client.with_default_headers(hosted::headers());
            }
            if let Some(adapter) = adapter {
                client = client.with_request_adapter(adapter);
            }
            Arc::new(client.with_timeouts(timeouts()).map_err(error)?)
        }
        ModelApi::Gemini => {
            let endpoint = format!("{}/", provider.endpoint.trim_end_matches('/'));
            let mut native = GeminiModel::new_with_base_url_and_timeouts(
                key,
                &config.model,
                endpoint,
                timeouts(),
            )
            .map_err(error)?
            .with_retry_config(RetryConfig::disabled())
            .with_request_retry(request_retry(provider))
            .map_err(error)?;
            if let Some(thinking) = reasoning::gemini(effort)? {
                native = native.with_thinking_config(thinking);
            }
            Arc::new(native)
        }
    };
    Ok(registry::Built::new(inner))
}

pub(super) fn error(error: impl std::borrow::Borrow<adk_core::AdkError>) -> Fault {
    let error = error.borrow();
    if let Some(fault) =
        std::error::Error::source(error).and_then(|source| source.downcast_ref::<Fault>())
    {
        return fault.clone();
    }
    let summary = summary(error);
    if error.component == adk_core::ErrorComponent::Model && error.code == "model.empty_response" {
        return Fault::new(ErrorCode::Unavailable, summary);
    }
    let message = error.message.trim();
    Fault::new(
        ErrorCode::Unavailable,
        if message.is_empty() {
            summary
        } else {
            format!("{summary}\n\n{message}")
        },
    )
}

fn summary(error: &adk_core::AdkError) -> String {
    if error.component == adk_core::ErrorComponent::Model && error.code == "model.empty_response" {
        return "Model returned no answer".into();
    }
    // Keep machine-readable classification separate from the native message.
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
    fn retains_status_and_native_message() {
        for (status, category) in [
            (400, ErrorCategory::InvalidInput),
            (401, ErrorCategory::Unauthorized),
            (429, ErrorCategory::RateLimited),
        ] {
            let mut failure = AdkError::new(
                ErrorComponent::Model,
                category,
                "model.openai_compat.api_error",
                "Provider rejected the current input",
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
                    "Agent execution failed (model.openai_compat.api_error); HTTP {status}; {category}\n\nProvider rejected the current input"
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
                "Provider rejected the current input",
            );
            assert_eq!(
                error(failure).message,
                format!("Agent execution failed ({code})\n\nProvider rejected the current input")
            );
        }
    }
}
