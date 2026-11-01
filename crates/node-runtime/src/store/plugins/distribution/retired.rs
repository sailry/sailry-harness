//! Formerly bundled packages whose behavior moved into the application retire once.
//! Their configuration is imported first; immutable captures keep the original package.
use super::*;
use sailry_protocol::{SessionConfig, WorkMode};

async fn retiring(db: &Connection, name: &str) -> Result<Option<Info>, Fault> {
    if crate::plugins::shipped(name) {
        return Ok(None);
    }
    Ok(get(db, name)
        .await?
        .filter(|info| info.origin == Some(plugin::Origin::Bundled)))
}

/// Whether startup must import retired configuration inside a backed-up update.
pub(in crate::store) async fn pending(db: &Connection) -> Result<bool, Fault> {
    for name in crate::plugins::RETIRED {
        if retiring(db, name).await?.is_some() {
            return Ok(true);
        }
    }
    Ok(!crate::store::teams::settings::configured(db).await?
        && crate::store::background_settings::retained(db)
            .await?
            .is_some())
}

pub(super) async fn retire(db: &Connection) -> Result<Vec<Event>, Fault> {
    let mut changes = vec![];
    for &name in crate::plugins::RETIRED {
        let Some(mut info) = retiring(db, name).await? else {
            continue;
        };
        for &(package, option, value, mode) in crate::plugins::RETIRED_MODES {
            if package == name {
                changes.extend(select(db, package, option, value, mode).await?);
            }
        }
        super::super::policy::import(db, &info).await?;
        info.summary.revision = next(db, name).await?;
        info.summary.enabled = false;
        connectors::remove_mcp(db, &info).await?;
        save(db, &info, false).await?;
        services::apply(db, &info, false).await?;
        crate::store::agent::continuation::invalidate(db, name).await?;
        changes.push(Event::PluginRemoved {
            name: name.to_owned(),
        });
    }
    super::super::policy::retained(db).await?;
    Ok(changes)
}

/// Clears the retired option; Standard work selecting it continues in the core mode.
fn convert(config: &mut SessionConfig, package: &str, option: &str, mode: WorkMode) {
    if let Some(options) = config.options.get_mut(package) {
        options.remove(option);
        if options.is_empty() {
            config.options.remove(package);
        }
    }
    if config.mode == WorkMode::Code {
        config.mode = mode;
    }
}

/// Current session and default selections move to a new revision; sent turns keep theirs.
async fn select(
    db: &Connection,
    package: &str,
    option: &str,
    value: &str,
    mode: WorkMode,
) -> Result<Vec<Event>, Fault> {
    let path = format!("$.options.\"{package}\".\"{option}\"");
    let mut changes = vec![];
    let mut after = i64::MIN;
    loop {
        let row: Option<(i64, String)> = {
            let mut statement = db.prepare("SELECT s.rowid,s.id FROM sessions s JOIN session_revisions r ON r.session=s.id AND r.revision=s.revision WHERE s.rowid>=?1 AND s.state!='removed' AND json_extract(r.config,?2)=?3 AND NOT EXISTS(SELECT 1 FROM agent_delegations d JOIN turns t ON t.id=d.turn WHERE t.session=s.id) ORDER BY s.rowid LIMIT 1").await.map_err(storage_error)?;
            match statement
                .query_row(params![after, path.as_str(), value])
                .await
            {
                Ok(row) => {
                    let mapped: turso::Result<_> =
                        (|row: &turso::Row| Ok((row.get::<i64>(0)?, row.get::<String>(1)?)))(&row);
                    Some(mapped.map_err(storage_error)?)
                }
                Err(turso::Error::QueryReturnedNoRows) => None,
                Err(error) => return Err(storage_error(error)),
            }
        };
        let Some((row, id)) = row else { break };
        let mut session =
            crate::store::commands::session(db, id.parse().map_err(storage_error)?).await?;
        convert(&mut session.config, package, option, mode);
        let (_, event) = crate::store::commands::revise_session(db, session).await?;
        changes.extend(event);
        let Some(next) = row.checked_add(1) else {
            break;
        };
        after = next;
    }
    let selected: bool = {
        let mut statement = db
            .prepare("SELECT EXISTS(SELECT 1 FROM defaults WHERE singleton=1 AND json_extract(config,?1)=?2)")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row(params![path.as_str(), value])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    if selected {
        let previous: Vec<u8> = {
            let mut statement = db
                .prepare("SELECT config FROM defaults WHERE singleton=1")
                .await
                .map_err(storage_error)?;
            let row = statement.query_row(()).await.map_err(storage_error)?;
            let mapped: turso::Result<_> = row.get(0);
            mapped.map_err(storage_error)?
        };
        let mut defaults = crate::store::commands::defaults(db).await?;
        let config = defaults
            .config
            .as_mut()
            .ok_or_else(|| storage_error("default conversation configuration is missing"))?;
        convert(config, package, option, mode);
        defaults.revision = defaults
            .revision
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or_else(|| Fault::invalid("configuration revision exhausted"))?;
        db.execute(
            "UPDATE defaults SET revision=?1,config=?2 WHERE singleton=1",
            params![
                defaults.revision as i64,
                super::super::options::preserve(Some(&previous), config)?
            ],
        )
        .await
        .map_err(storage_error)?;
        changes.push(Event::DefaultsChanged(defaults));
    }
    Ok(changes)
}

#[cfg(test)]
mod tests;
