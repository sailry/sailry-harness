//! Public selection is checked before configuration or turn admission commits.
use super::*;
use sailry_protocol::SessionConfig;

#[cfg(test)]
mod tests;

pub(in crate::store) async fn validate(
    db: &Connection,
    config: &SessionConfig,
) -> Result<Option<plugin::Reference>, Fault> {
    let mut packages = Vec::new();
    for name in config.options.keys() {
        let Some(info) = get(db, name).await? else {
            continue;
        };
        if info.extension.as_ref().is_some_and(|extension| {
            extension
                .conversation_modes
                .iter()
                .any(|mode| mode.selected(config.options.get(name)))
        }) && !info.summary.available()
        {
            crate::plugins::compatibility::check(&info)?;
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "conversation mode is unavailable",
            ));
        }
        packages.push(info);
    }
    let selected = crate::plugins::modes::selected(&packages, config)?
        .map(|(package, _)| package.summary.reference());
    if let Some(mode) = &selected
        && config
            .assistant
            .as_ref()
            .is_some_and(|binding| binding.package.name == mode.name && binding.package != *mode)
    {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "conversation mode package revision differs from the assistant",
        ));
    }
    Ok(selected)
}

pub(in crate::store) async fn clear(
    db: &Connection,
    config: &mut SessionConfig,
) -> Result<(), Fault> {
    for (name, options) in &mut config.options {
        let Some(info) = get(db, name).await? else {
            continue;
        };
        for mode in info
            .extension
            .iter()
            .flat_map(|extension| &extension.conversation_modes)
        {
            if mode.selected(Some(options)) {
                options.insert(
                    mode.option.clone(),
                    serde_json::Value::String(String::new()),
                );
            }
        }
    }
    Ok(())
}
