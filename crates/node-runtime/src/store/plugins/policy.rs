//! Child budgets configured before the execution Node owned them import once.
use super::*;

#[cfg(test)]
pub(in crate::store) mod fixture;
#[cfg(test)]
mod tests;

fn field(info: &Info) -> Option<&str> {
    info.settings
        .iter()
        .flat_map(|schema| &schema.properties)
        .find(|(_, field)| field.child_concurrency)
        .map(|(name, _)| name.as_str())
}

/// Imports a retiring package's current budget before the package is removed.
pub(super) async fn import(db: &Connection, info: &Info) -> Result<(), Fault> {
    let Some(name) = field(info) else {
        return Ok(());
    };
    let state = settings::read_admitted(db, info).await?;
    let Some(limit) = state
        .values
        .get(name)
        .and_then(|value| crate::plugins::settings::child_limit(value).ok())
    else {
        return Ok(());
    };
    crate::store::teams::settings::import(db, limit).await
}

/// Imports the value retained from the earlier native background setting.
pub(super) async fn retained(db: &Connection) -> Result<(), Fault> {
    if let Some(limit) = super::super::background_settings::retained(db).await? {
        crate::store::teams::settings::import(db, limit).await?;
    }
    Ok(())
}
