use super::*;
mod authorization;
mod call;
pub(in crate::agent) use call::{is_summary, summary_retry, summary_scope};
mod diagnostics;
mod errors;
mod factory;
pub(super) mod history;
mod legacy;
pub(super) mod observation;
pub(super) mod plugin;
pub(crate) mod registry;
pub(super) mod timing;
use adk_core::Llm;
pub(super) use diagnostics::cause;
use goose_provider_types::{base::Provider as GdkProvider, model::ModelConfig};
use sailry_protocol::{
    Authentication, Effort, SessionConfig,
    conversation::{ModelApi, Provider},
};

pub(super) struct Binding {
    pub provider: Arc<dyn GdkProvider>,
    pub config: ModelConfig,
}

#[derive(Clone, Copy)]
pub(super) enum Purpose {
    Main,
    Helper,
    Compaction,
}

pub(super) async fn provider(
    ingress: &Arc<Ingress>,
    invocation: &Invocation,
    stop: &CancellationToken,
    _purpose: Purpose,
) -> Result<Binding, Fault> {
    let provider = invocation.provider.as_ref().ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "provider was not configured when this turn was queued",
        )
    })?;
    binding(
        ingress,
        provider,
        &invocation.turn.config,
        invocation.turn.session,
        invocation.child.is_some(),
        stop,
    )
    .await
}

pub(super) async fn binding(
    ingress: &Arc<Ingress>,
    provider: &Provider,
    config: &SessionConfig,
    session: sailry_protocol::SessionId,
    delegated: bool,
    stop: &CancellationToken,
) -> Result<Binding, Fault> {
    if !provider.enabled
        || (!provider.models.is_empty()
            && !provider.models.iter().any(|model| model.id == config.model))
    {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "provider or model was unavailable when this turn was sent",
        ));
    }
    let model_config = factory::config(provider, config);
    let inner: Arc<dyn GdkProvider> = if provider.api == ModelApi::Plugin {
        Arc::new(plugin::Model::new(ingress, provider, config, session, stop).await?)
    } else if provider.authentication == Authentication::ChatGpt {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "GDK does not support this ChatGPT authorization endpoint",
        ));
    } else if provider.authentication == Authentication::ApiKey {
        let credential = match &config.credential {
            Some(reference) => Some(
                ingress
                    .model_credential(reference.clone(), config.provider)
                    .await?,
            ),
            None => None,
        };
        let revision = credential
            .as_ref()
            .map(|(credential, _)| credential.revision);
        let fingerprint = registry::fingerprint(provider, config, revision)?;
        let key = credential
            .as_ref()
            .map_or("", |(_, secret)| secret.expose());
        let inner = ingress.model_clients.get(session, fingerprint, || {
            factory::native(
                &ingress.model_clients,
                provider,
                &config.model,
                session,
                key,
                reqwest::header::HeaderMap::new(),
            )
        })?;
        errors::wrap(
            inner,
            credential.into_iter().map(|(_, secret)| secret).collect(),
        )
    } else {
        crate::providers::login::validate(provider)?;
        Arc::new(authorization::Model::new(
            ingress, provider, config, session, delegated, stop,
        ))
    };
    Ok(Binding {
        provider: inner,
        config: model_config,
    })
}

// Only this boundary remains ADK-shaped until the Stage 4 state-machine switch.
pub(super) async fn build(
    ingress: &Arc<Ingress>,
    invocation: &Invocation,
    stop: &CancellationToken,
    purpose: Purpose,
) -> Result<Arc<dyn Llm>, Fault> {
    let binding = provider(ingress, invocation, stop, purpose).await?;
    let captured = invocation.provider.as_ref().unwrap();
    let metadata = captured
        .models
        .iter()
        .find(|model| model.id == invocation.turn.config.model);
    Ok(Arc::new(attachments::Model {
        inner: Arc::new(legacy::Model {
            binding,
            stop: stop.clone(),
        }),
        ingress: ingress.clone(),
        turn: invocation.turn.id,
        vision: metadata.is_none_or(|model| model.vision)
            && captured
                .binding
                .as_ref()
                .and_then(|binding| binding.adapter.as_ref())
                .is_none_or(|adapter| adapter.capabilities.vision),
        api: attachments::api(captured, captured.api),
        window: history::Window::new(captured, &invocation.turn.config.model),
        tools: metadata.is_none_or(|model| model.tools)
            && captured
                .binding
                .as_ref()
                .and_then(|binding| binding.adapter.as_ref())
                .is_none_or(|adapter| adapter.capabilities.tools),
        stop: stop.clone(),
        identity: crate::logging::Context {
            node: Some(ingress.node),
            session: Some(invocation.turn.session),
            turn: Some(invocation.turn.id),
            provider: Some(captured.id),
            provider_name: Some(captured.name.clone()),
            model: Some(invocation.turn.config.model.clone()),
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
    Ok(Arc::new(legacy::Model {
        binding: binding(ingress, provider, config, session, delegated, stop).await?,
        stop: stop.clone(),
    }))
}

pub(in crate::agent) fn context_overflow(error: &adk_core::AdkError) -> bool {
    error.code == "model.sailry.context_overflow"
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
