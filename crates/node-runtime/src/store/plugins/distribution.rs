//! Missing defaults and bundled settings refresh reuse ordinary installation.
use super::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(in crate::store) fn install(
    db: &mut Connection,
    host: &crate::plugins::Host,
) -> Result<(), Fault> {
    let transaction = db.transaction().map_err(storage_error)?;
    initialize(&transaction, host)?;
    transaction.commit().map_err(storage_error)
}

pub(in crate::store) fn initialize(
    db: &Connection,
    host: &crate::plugins::Host,
) -> Result<(), Fault> {
    for &(name, enabled) in crate::plugins::DEFAULTS {
        let recorded: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plugins WHERE name=?1)",
                [name],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if recorded {
            continue;
        }
        let info = host.install_bundled(name)?;
        let enabled = services::initial(db, &info)?.unwrap_or(enabled);
        let mut result = Ok(Output::Plugin(info));
        let mut event = finish(
            db,
            &Command::InstallBundledPlugin {
                name: name.into(),
                expected_revision: 0,
            },
            &mut result,
        )?;
        if let Output::Plugin(mut info) = result?
            && info.summary.enabled != enabled
        {
            info.summary.enabled = enabled;
            save(db, &info, true)?;
            event = Some(Event::PluginChanged(info.summary));
        }
        if let Some(event) = event {
            db.execute("INSERT INTO events(body) VALUES(?1)", [encode(&event)?])
                .map_err(storage_error)?;
        }
    }
    refresh(db, host)?;
    crate::store::providers::authentication::initialize(db)?;
    db.execute("UPDATE node SET plugin_defaults=1 WHERE singleton=1", [])
        .map_err(storage_error)?;
    Ok(())
}

fn refresh(db: &Connection, host: &crate::plugins::Host) -> Result<(), Fault> {
    for summary in list(db)? {
        let name = summary.name.as_str();
        let Some(current) = get(db, name)? else {
            continue;
        };
        if current.origin != Some(plugin::Origin::Bundled) {
            continue;
        }
        let newer =
            match crate::plugins::Host::bundled_newer(name, current.summary.version.as_deref()) {
                Ok(newer) => newer,
                Err(error) if error.code == ErrorCode::NotFound => continue,
                Err(error) => return Err(error),
            };
        if !newer {
            continue;
        }
        let mut result = Ok(Output::Plugin(host.install_bundled(name)?));
        let event = finish(
            db,
            &Command::InstallBundledPlugin {
                name: name.into(),
                expected_revision: current.summary.revision,
            },
            &mut result,
        )?;
        result?;
        if let Some(event) = event {
            db.execute("INSERT INTO events(body) VALUES(?1)", [encode(&event)?])
                .map_err(storage_error)?;
        }
    }
    Ok(())
}
