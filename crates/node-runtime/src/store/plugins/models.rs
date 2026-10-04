//! Model selection uses the execution Node's configured providers.
use super::*;
use sailry_protocol::{
    Effort, Permission, SessionConfig, WorkMode, conversation::Provider, plugin::models::*,
};

pub(super) fn catalog(db: &Connection) -> Result<Catalog, Fault> {
    let providers = super::super::agent::providers::list(db)?;
    let models: Vec<_> = providers
        .iter()
        .filter(|provider| provider.enabled)
        .flat_map(|provider| {
            provider.models.iter().map(move |model| Model {
                kind: Kind::Provider,
                id: format!("{}/{}", provider.id, model.id),
                provider: provider.name.clone(),
                model: model.id.clone(),
                default: provider.default_model == model.id,
                reasoning: model.reasoning,
                efforts: if model.reasoning {
                    model.efforts.clone()
                } else {
                    vec![]
                },
                default_effort: model.default_effort,
            })
        })
        .collect();
    Ok(Catalog { models })
}

pub(super) fn resolve(db: &Connection, selected: &str) -> Result<(Provider, SessionConfig), Fault> {
    if selected.is_empty() {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "plugin model is not configured",
        ));
    }
    let (provider, model) = selected
        .split_once('/')
        .ok_or_else(|| Fault::new(ErrorCode::InvalidRequest, "invalid plugin model selection"))?;
    let provider = provider
        .parse()
        .map_err(|_| Fault::new(ErrorCode::InvalidRequest, "invalid plugin model provider"))?;
    let provider = super::super::agent::providers::get(db, provider)?
        .filter(|provider| provider.enabled)
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::NotConfigured,
                "selected model provider is unavailable",
            )
        })?;
    let model = provider
        .models
        .iter()
        .find(|candidate| candidate.id == model)
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "selected model is unavailable"))?;
    let config = SessionConfig {
        assistant: None,
        resource: None,
        provider: provider.id,
        model: model.id.clone(),
        effort: Effort::initial(&model.efforts, model.default_effort),
        mode: WorkMode::Code,
        permission: Permission::Ask,
        credential: provider.credential.clone(),
    };
    Ok((provider, config))
}

pub(super) fn session(
    db: &Connection,
    selected: &str,
    effort: Option<Effort>,
    base: Option<&SessionConfig>,
) -> Result<SessionConfig, Fault> {
    if base.is_some_and(|config| config.assistant.is_some() || config.resource.is_some()) {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "model selection requires a project or unassigned scope",
        ));
    }
    let (provider, resolved) = resolve(db, selected)?;
    let mut config = base.cloned().unwrap_or_else(|| resolved.clone());
    let unchanged = config.provider == resolved.provider && config.model == resolved.model;
    config.provider = resolved.provider;
    config.model = resolved.model;
    config.credential = resolved.credential;
    config.effort = effort.unwrap_or(if unchanged {
        config.effort
    } else {
        resolved.effort
    });
    super::super::agent::providers::selection(&provider, &config)?;
    Ok(config)
}
