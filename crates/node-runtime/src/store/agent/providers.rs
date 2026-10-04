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
    } else {
        config.effort.validate(provider.api, 0)?;
    }
    Ok(())
}

pub(in crate::store) fn put(
    db: &Connection,
    node: NodeId,
    provider: &Provider,
    expected: u64,
) -> Result<Provider, Fault> {
    validate(db, node, provider)?;
    let revision = get(db, provider.id)?.map_or(0, |provider| provider.revision);
    if revision != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "provider revision changed",
        ));
    }
    let mut provider = provider.clone();
    super::super::providers::completion::fill(db, &mut provider)?;
    validate(db, node, &provider)?;
    provider.revision = revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| invalid("provider revision exhausted"))?;
    provider.endpoint = crate::providers::endpoint_url(&provider.endpoint)?
        .as_str()
        .trim_end_matches('/')
        .to_owned();
    crate::store::providers::configuration::publish(db, &provider)?;
    Ok(provider)
}

pub(in crate::store) fn validate(
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
        )?;
    }
    if encode(provider)?.len() > 64 * 1024 {
        return Err(invalid("provider configuration exceeds its size limit"));
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
        {
            return Err(invalid("model configuration is invalid or unsupported"));
        }
        model.validate_reasoning(provider.api)?;
    }
    if !provider.models.is_empty() && !ids.contains(&provider.default_model) {
        return Err(invalid("default model is not in this provider"));
    }
    if provider.name.trim().is_empty()
        || provider.name.len() > 128
        || provider.endpoint.len() > 2048
    {
        return Err(invalid(
            "provider name and endpoint are required and must be bounded",
        ));
    }
    crate::providers::endpoint_url(&provider.endpoint)?;
    crate::providers::cloud::validate(provider)?;
    crate::providers::login::validate(provider)?;
    Ok(())
}

pub(in crate::store) fn remove(
    db: &Connection,
    id: ProviderId,
    expected: u64,
) -> Result<(), Fault> {
    let provider =
        get(db, id)?.ok_or_else(|| Fault::new(ErrorCode::NotFound, "provider does not exist"))?;
    if provider.revision != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "provider revision changed",
        ));
    }
    crate::store::providers::configuration::unlist(db, id)
}
