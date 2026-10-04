//! New turns capture the live Node inventory; admitted turns retain immutable references.
use super::*;

pub(in crate::store) fn assistant(
    db: &Connection,
    binding: &plugin::conversation::Binding,
) -> Result<Vec<plugin::Reference>, Fault> {
    validate(db, &binding.package)?;
    let owner = packages(db, std::slice::from_ref(&binding.package))?;
    let declaration = crate::plugins::conversation::declaration(binding, &owner)?;
    let mut references = vec![binding.package.clone()];
    for tool in &declaration.tools {
        let plugin::conversation::Tool::Package { package, name } = tool else {
            continue;
        };
        let unavailable = || {
            Fault::new(
                ErrorCode::NotConfigured,
                "assistant package tool is unavailable",
            )
        };
        let info = if package == &binding.package.name {
            owner[0].clone()
        } else {
            crate::store::plugins::get(db, package)?
                .filter(|info| info.summary.enabled)
                .ok_or_else(unavailable)?
        };
        if crate::plugins::tools::computer(&info, name).is_none()
            && !info.extension.as_ref().is_some_and(|extension| {
                extension.tools.iter().any(|tool| {
                    tool.name == *name
                        && tool.server.is_none()
                        && (tool.operation.is_some() || tool.handler.is_some())
                })
            })
        {
            return Err(unavailable());
        }
        let reference = info.summary.reference();
        if !references.contains(&reference) {
            references.push(reference);
        }
    }
    Ok(references)
}

pub(in crate::store) fn capture(db: &Connection) -> Result<Vec<plugin::Reference>, Fault> {
    let mut references = Vec::new();
    for summary in crate::store::plugins::list(db)? {
        if !summary.enabled {
            continue;
        }
        let Some(info) = crate::store::plugins::get(db, &summary.name)? else {
            continue;
        };
        if info
            .extension
            .as_ref()
            .is_some_and(|extension| extension.scope == plugin::Scope::Desktop)
        {
            continue;
        }
        // Background and UI-only packages are not Agent dependencies. Opening
        // a session must not capture unrelated pages or callback packages.
        if info.skills.is_empty()
            && info.mcp.is_empty()
            && !crate::plugins::tools::owns_computer(&info)
            && info.extension.as_ref().is_none_or(|extension| {
                extension.tools.is_empty() && extension.model_tools.is_empty()
            })
        {
            continue;
        }
        references.push(summary.reference());
    }
    Ok(references)
}

pub(in crate::store) fn validate(
    db: &Connection,
    reference: &plugin::Reference,
) -> Result<(), Fault> {
    let info = crate::store::plugins::get(db, &reference.name)?;
    if !info.is_some_and(|info| info.summary.enabled) {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "assistant plugin is disabled or removed",
        ));
    }
    packages(db, std::slice::from_ref(reference))?;
    Ok(())
}

/// Admitted work may still use a version after inventory update, disable or removal.
pub(in crate::store) fn packages(
    db: &Connection,
    references: &[plugin::Reference],
) -> Result<Vec<plugin::Info>, Fault> {
    references
        .iter()
        .map(|reference| {
            let body: Option<Vec<u8>> = db
                .query_row(
                    "SELECT body FROM plugin_packages WHERE digest=?1 AND name=?2",
                    params![reference.digest, reference.name],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            let mut info: plugin::Info = serde_json::from_slice(&body.ok_or_else(|| {
                Fault::new(
                    ErrorCode::NotConfigured,
                    "plugin version is unavailable on this Node",
                )
            })?)
            .map_err(storage_error)?;
            crate::store::plugins::settings::validate_revision(db, reference)?;
            info.summary.settings_revision = reference.settings_revision;
            Ok(info)
        })
        .collect()
}

/// Read the exact references frozen when this turn was admitted.
pub(in crate::store) fn read(
    db: &Connection,
    turn: TurnId,
) -> Result<Vec<plugin::Reference>, Fault> {
    let body: Vec<u8> = db
        .query_row(
            "SELECT plugins FROM turns WHERE id=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    serde_json::from_slice(&body).map_err(storage_error)
}
