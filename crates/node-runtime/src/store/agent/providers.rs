use super::*;
pub(in crate::store) use crate::store::providers::configuration::{get, list};

pub(in crate::store) fn selection(
    provider: &Provider,
    config: &SessionConfig,
) -> Result<(), Fault> {
    if let Some(model) = provider
        .models
        .iter()
        .find(|model| model.id == config.model)
    {
        model.validate_effort(provider.api, config.effort)?;
        model.validate_features(provider.api, &config.features)?;
    } else {
        if !config.features.is_empty() {
            return Err(Fault::invalid(
                "model feature choices require an advertised model",
            ));
        }
        config.effort.validate(provider.api, 0)?;
    }
    Ok(())
}

pub(in crate::store) async fn put(
    db: &Connection,
    node: NodeId,
    provider: &Provider,
    expected: u64,
) -> Result<Provider, Fault> {
    if provider.binding.is_some() && !super::super::providers::presets::partner_policy(provider)? {
        return Err(Fault::invalid(
            "captured channel bindings cannot be saved as configuration",
        ));
    }
    validate(db, node, provider).await?;
    let previous = get(db, provider.id).await?;
    let revision = previous.as_ref().map_or(0, |provider| provider.revision);
    if revision != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "provider revision changed",
        ));
    }
    let mut provider = provider.clone();
    super::super::providers::presets::save_policy(db, &mut provider, previous.as_ref()).await?;
    super::super::providers::completion::fill(db, &mut provider).await?;
    validate(db, node, &provider).await?;
    provider.revision = revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| sailry_protocol::Fault::invalid("provider revision exhausted"))?;
    provider.endpoint = crate::providers::endpoint_url(&provider.endpoint)?
        .as_str()
        .trim_end_matches('/')
        .to_owned();
    crate::store::providers::configuration::publish(db, &provider).await?;
    Ok(provider)
}

pub(in crate::store) async fn validate(
    db: &Connection,
    node: NodeId,
    provider: &Provider,
) -> Result<(), Fault> {
    if let Some(reference) = &provider.credential {
        if reference.node != node {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "credential belongs to another Node",
            ));
        }
        crate::store::providers::authentication::validate_authentication(
            db,
            reference.id,
            provider.id,
            provider.authentication,
        )
        .await?;
    }
    if encode(provider)?.len() > 64 * 1024 {
        return Err(sailry_protocol::Fault::invalid(
            "provider configuration exceeds its size limit",
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    for model in &provider.models {
        if model.id.trim().is_empty()
            || model.id.len() > 256
            || !ids.insert(&model.id)
            || model.context == 0
            || model.output == 0
            || model.output > model.context
            || model.output > i32::MAX as u32
            || model.compaction_tokens == Some(0)
        {
            return Err(sailry_protocol::Fault::invalid(
                "model configuration is invalid or unsupported",
            ));
        }
        model.validate_reasoning(provider.api)?;
        model.validate_features(provider.api, &Default::default())?;
    }
    if !provider.models.is_empty() && !ids.contains(&provider.default_model) {
        return Err(sailry_protocol::Fault::invalid(
            "default model is not in this provider",
        ));
    }
    if provider.name.trim().is_empty()
        || provider.name.len() > 128
        || provider.endpoint.len() > 2048
    {
        return Err(sailry_protocol::Fault::invalid(
            "provider name and endpoint are required and must be bounded",
        ));
    }
    crate::providers::endpoint_url(&provider.endpoint)?;
    if provider.binding.is_some() {
        crate::store::providers::presets::verify(db, provider).await?;
    } else {
        crate::store::providers::presets::validate(db, provider).await?;
    }
    crate::providers::cloud::validate(provider)?;
    crate::providers::login::validate(provider)?;
    Ok(())
}

pub(in crate::store) async fn remove(
    db: &Connection,
    id: ProviderId,
    expected: u64,
) -> Result<(), Fault> {
    let provider = get(db, id)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "provider does not exist"))?;
    if provider.revision != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "provider revision changed",
        ));
    }
    crate::store::providers::configuration::unlist(db, id).await
}
