use super::*;
mod authorization;
mod call;
pub(in crate::agent) use call::{is_summary, summary_retry, summary_scope};
mod cloud;
mod diagnostics;
pub(super) mod effort;
mod errors;
mod features;
pub(super) mod history;
mod hosted;
pub(super) mod observation;
mod opencode;
pub(super) mod plugin;
mod reasoning;
pub(crate) mod registry;
mod retry;
mod streaming;
pub(super) mod timing;
mod tool_choice;
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
) -> Result<Built, Fault> {
    let provider = invocation.provider.as_ref().ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "provider was not configured when this turn was queued",
        )
    })?;
    let config = &invocation.turn.config;
    let Built {
        model: inner,
        configuration,
    } = configure(
        ingress,
        provider,
        config,
        invocation.turn.session,
        Some(invocation.turn.id),
        invocation.child.is_some(),
        stop,
        purpose,
    )
    .await?;
    let metadata = provider
        .models
        .iter()
        .find(|model| model.id == config.model);
    Ok(Built {
        model: Arc::new(attachments::Model {
            inner: Arc::new(retry::Model {
                inner,
                ingress: ingress.clone(),
                turn: Some(invocation.turn.id),
                session: invocation.turn.session,
                stop: stop.clone(),
                limit: provider.retry_limit(),
                compaction: matches!(purpose, Purpose::Compaction),
            }),
            ingress: ingress.clone(),
            turn: invocation.turn.id,
            vision: metadata.is_none_or(|model| model.vision)
                && provider
                    .binding
                    .as_ref()
                    .and_then(|binding| binding.adapter.as_ref())
                    .is_none_or(|adapter| adapter.capabilities.vision),
            api: attachments::api(provider, opencode::api(provider.api, &config.model)?),
            window: history::Window::new(provider, &config.model),
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
        }),
        configuration,
    })
}

pub(super) struct Built {
    pub model: Arc<dyn Llm>,
    pub configuration: Option<effort::Capture>,
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
        None,
        delegated,
        stop,
        Purpose::Helper,
    )
    .await?;
    Ok(Arc::new(retry::Model {
        inner: inner.model,
        ingress: ingress.clone(),
        turn: None,
        session,
        stop: stop.clone(),
        limit: provider.retry_limit(),
        compaction: false,
    }))
}

#[derive(Clone, Copy)]
pub(super) enum Purpose {
    Main,
    Helper,
    Compaction,
}

async fn configure(
    ingress: &Arc<Ingress>,
    provider: &Provider,
    config: &SessionConfig,
    session: sailry_protocol::SessionId,
    turn: Option<TurnId>,
    delegated: bool,
    stop: &CancellationToken,
    purpose: Purpose,
) -> Result<Built, Fault> {
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
    let mut configuration = None;
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
        let origin = history::origin(
            provider,
            &config.model,
            credential.as_ref().map(|(credential, _)| credential),
            None,
        );
        configuration = effort::Capture::load(
            ingress,
            turn,
            session,
            origin.clone(),
            effort,
            effort::supported(provider, metadata),
        )
        .await?;
        let resolved = match &configuration {
            Some(capture) => reasoning::resolve(metadata, api, capture.base())?,
            None => resolved,
        };
        let effort = resolved.effort;
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
        history::wrap(
            errors::wrap(
                entry.clone(),
                credential.into_iter().map(|(_, secret)| secret).collect(),
            ),
            origin,
            history::Window::new(provider, &config.model),
        )
    } else {
        crate::providers::login::validate(provider)?;
        let (model, captured) = authorization::Model::new(
            ingress,
            provider,
            config,
            authorization::Context {
                session,
                turn,
                delegated,
                reasoning: resolved,
                purpose,
                stop: stop.clone(),
            },
        )
        .await?;
        configuration = captured;
        let model = Arc::new(model);
        if matches!(purpose, Purpose::Main) {
            let _ = model.preconnect().await;
        }
        model as Arc<dyn Llm>
    };
    Ok(Built {
        model: if api == ModelApi::Responses {
            features::wrap(
                effort::wrap(inner, configuration.clone()),
                config.features.clone(),
            )
        } else {
            inner
        },
        configuration,
    })
}

pub(in crate::agent) fn context_overflow(error: &adk_core::AdkError) -> bool {
    retry::context_overflow(error)
}

fn request_retry() -> adk_model::request_retry::RequestRetryConfig {
    // The outer model owns retries across HTTP and stream failures.
    adk_model::request_retry::RequestRetryConfig::default()
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
    let injection = provider
        .binding
        .as_ref()
        .filter(|_| provider.authentication == Authentication::ApiKey)
        .and_then(|binding| binding.key_injection.as_ref());
    let adapter = match injection {
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
    let key = if injection.is_some() { "" } else { key };
    let adapter = tool_choice::adapter(provider.api, adapter);
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
            cloud::keyed(provider, &config.model, key, effort, adapter)?
        }
        ModelApi::DeepSeek => {
            let mut client = DeepSeekClient::new(reasoning::deepseek(
                DeepSeekConfig::new(key, &config.model).with_base_url(&provider.endpoint),
                effort,
            )?)
            .map_err(error)?
            .with_retry_config(RetryConfig::disabled())
            .with_request_retry(request_retry())
            .with_timeouts(timeouts())
            .map_err(error)?;
            if let Some(adapter) = adapter {
                client = client.with_request_adapter(adapter);
            }
            Arc::new(client)
        }
        ModelApi::ChatCompletions => {
            let mut client = OpenAICompatible::new_with_reasoning_effort(
                OpenAICompatibleConfig::new(key, &config.model).with_base_url(&provider.endpoint),
                reasoning::openai(effort)?,
            )
            .map_err(error)?
            .with_retry_config(RetryConfig::disabled())
            .with_request_retry(request_retry());
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
                .with_request_retry(request_retry())
                .with_response_recovery(true);
            if provider.authentication == Authentication::ApiKey {
                client = client
                    .with_default_headers(hosted::headers())
                    .map_err(error)?;
            }
            client = client.with_request_adapter(streaming::adapter(adapter, websocket.as_ref()));
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
                .with_request_retry(request_retry());
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
            .with_request_retry(request_retry())
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
    use adk_core::{ErrorCategory, ErrorComponent};
    use sailry_protocol::{FaultDetails, ModelFailure};
    let error = error.borrow();
    if let Some(fault) =
        std::error::Error::source(error).and_then(|source| source.downcast_ref::<Fault>())
    {
        return fault.clone();
    }
    let mut fault = Fault::new(ErrorCode::Unavailable, error.message.clone());
    if error.component == ErrorComponent::Model {
        let provider_code = error
            .details
            .metadata
            .get("provider_error_code")
            .and_then(serde_json::Value::as_str);
        let reason = match provider_code {
            Some("subscription_sharing_usage_limit_exceeded") => ModelFailure::SharedUsageLimit,
            Some("subscription_sharing_usage_unavailable") => ModelFailure::SharedUsageUnavailable,
            _ if matches!(
                error.code,
                "model.openai_responses.parse"
                    | "model.openai_compat.parse"
                    | "model.azure_openai.parse"
                    | "model.http.decode"
                    | "model.plugin.invalid_translation"
            ) =>
            {
                ModelFailure::InvalidResponse
            }
            _ => match error.category {
                ErrorCategory::Unauthorized | ErrorCategory::Forbidden => {
                    ModelFailure::Unauthorized
                }
                ErrorCategory::RateLimited => ModelFailure::RateLimited,
                ErrorCategory::Timeout => ModelFailure::Timeout,
                _ => ModelFailure::Request,
            },
        };
        fault.details = Some(FaultDetails::ModelRequest { reason });
    }
    fault
}

#[cfg(test)]
mod error_tests {
    use super::*;
    use adk_core::{AdkError, ErrorCategory, ErrorComponent};

    #[test]
    fn retains_status_and_native_message() {
        use sailry_protocol::{FaultDetails, ModelFailure};
        for (status, category, reason) in [
            (400, ErrorCategory::InvalidInput, ModelFailure::Request),
            (401, ErrorCategory::Unauthorized, ModelFailure::Unauthorized),
            (429, ErrorCategory::RateLimited, ModelFailure::RateLimited),
            (408, ErrorCategory::Timeout, ModelFailure::Timeout),
            (503, ErrorCategory::Unavailable, ModelFailure::Request),
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
            let fault = error(&failure);
            assert_eq!(fault.details, Some(FaultDetails::ModelRequest { reason }));
            assert_eq!(fault.message, "Provider rejected the current input");
            assert!(!fault.message.contains("private"));
            let mut entry = crate::logging::Entry::new(
                crate::logging::Area::Model,
                crate::logging::Phase::Failed,
            );
            diagnostics::cause(&mut entry, &failure);
            assert_eq!(entry.upstream_status, Some(u64::from(status)));
            assert_eq!(entry.cause_code, Some("model.openai_compat.api_error"));
        }
    }

    #[test]
    fn preserves_shared_usage_codes() {
        use sailry_protocol::{FaultDetails, ModelFailure};
        for (code, reason) in [
            (
                "subscription_sharing_usage_limit_exceeded",
                ModelFailure::SharedUsageLimit,
            ),
            (
                "subscription_sharing_usage_unavailable",
                ModelFailure::SharedUsageUnavailable,
            ),
        ] {
            let mut failure = AdkError::new(
                ErrorComponent::Model,
                ErrorCategory::Forbidden,
                "model.openai_responses.ws_error",
                "Provider rejected the current input",
            );
            failure
                .details
                .metadata
                .insert("provider_error_code".into(), code.into());
            let fault = error(failure);
            assert_eq!(fault.details, Some(FaultDetails::ModelRequest { reason }));
            assert_eq!(fault.message, "Provider rejected the current input");
        }
    }

    #[test]
    fn preserves_execution_and_source_faults() {
        use sailry_protocol::FaultDetails;
        let failure = AdkError::agent("Max iterations (100) exceeded");
        let fault = error(failure);
        assert_eq!(fault.message, "Max iterations (100) exceeded");
        assert!(fault.details.is_none());
        for details in [
            FaultDetails::ModelRecovery {
                reason: sailry_protocol::conversation::recovery::Stop::RateLimited,
            },
            FaultDetails::StorageConflict,
        ] {
            let mut fault = Fault::new(ErrorCode::Busy, "Original failure");
            fault.details = Some(details);
            let failure = AdkError::model("Wrapper message").with_source(fault.clone());
            assert_eq!(error(failure), fault);
        }
    }

    #[test]
    fn identifies_parse_failures_by_code() {
        let failure = AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::Internal,
            "model.openai_responses.parse",
            "Malformed upstream event",
        );
        assert_eq!(
            error(failure).details,
            Some(sailry_protocol::FaultDetails::ModelRequest {
                reason: sailry_protocol::ModelFailure::InvalidResponse,
            })
        );
    }

    #[test]
    fn preserves_native_text() {
        for message in [
            "",
            "  upstream failure\nsecond line  ",
            "{\"error\":\"rejected\"}",
        ] {
            let failure = AdkError::new(
                ErrorComponent::Model,
                ErrorCategory::Unavailable,
                "model.fixture.failed",
                message,
            );
            assert_eq!(error(failure).message, message);
        }
    }
}
