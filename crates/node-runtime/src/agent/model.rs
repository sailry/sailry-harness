use super::*;
mod authorization;
mod errors;
mod factory;
mod names;
pub(in crate::agent) mod observation;
pub(super) mod plugin;
pub(crate) mod registry;
mod request;
#[cfg(test)]
mod rig;
pub(in crate::agent) mod summary;
#[cfg(test)]
mod types;
pub(in crate::agent) use errors::{fault as error, safe};
use goose_provider_types::{base::Provider as GdkProvider, model::ModelConfig};
use sailry_protocol::{
    Authentication, Effort, SessionConfig,
    conversation::{ModelApi, Provider},
};

pub(super) struct Binding {
    pub provider: Arc<dyn GdkProvider>,
    pub config: ModelConfig,
    pub sources: observation::Sources,
    pub names: names::Registry,
}

#[derive(Clone, Copy)]
pub(super) enum Purpose {
    Main,
    Compaction,
}

pub(super) async fn provider(
    ingress: &Arc<Ingress>,
    invocation: &Invocation,
    stop: &CancellationToken,
    purpose: Purpose,
) -> Result<Binding, Fault> {
    let provider = invocation.provider.as_ref().ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "provider was not configured when this turn was queued",
        )
    })?;
    let compaction = matches!(purpose, Purpose::Compaction);
    // An absent supported effort must not resolve back to the main model's
    // default. This request-local copy preserves captured identity and policy.
    let summary_provider = compaction.then(|| {
        let mut provider = provider.clone();
        if let Some(model) = provider
            .models
            .iter_mut()
            .find(|model| model.id == invocation.turn.config.model)
        {
            model.default_effort = sailry_protocol::Effort::Default;
        }
        provider
    });
    let provider = summary_provider.as_ref().unwrap_or(provider);
    let config = if compaction {
        summary::config(provider, &invocation.turn.config)
    } else {
        invocation.turn.config.clone()
    };
    let mut binding = binding(
        ingress,
        provider,
        &config,
        invocation.turn.session,
        invocation.child.is_some(),
        stop,
    )
    .await?;
    if compaction {
        // Both GDK summary paths use fixed prompts without Node owner parts.
        binding.sources.replace(Vec::new());
        binding.config.max_tokens = Some(summary::max_tokens(provider, &config));
    }
    binding.provider = request::Model::wrap(
        binding.provider,
        ingress,
        invocation,
        purpose,
        stop,
        binding.sources.clone(),
        binding.names.clone(),
    );
    Ok(binding)
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
    let model_config = factory::config(provider, config, session);
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
        sources: observation::Sources::default(),
        names: names::Registry::default(),
    })
}
