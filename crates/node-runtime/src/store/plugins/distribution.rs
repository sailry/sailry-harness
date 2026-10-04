//! Initial package distribution reuses ordinary installation and immutable metadata.
use super::*;

#[cfg(test)]
mod tests;

pub(in crate::store) fn install(
    db: &mut Connection,
    host: &crate::plugins::Host,
) -> Result<(), Fault> {
    let initialized: bool = db
        .query_row(
            "SELECT plugin_defaults FROM node WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if initialized {
        return Ok(());
    }
    let transaction = db.transaction().map_err(storage_error)?;
    for &(name, enabled) in crate::plugins::DEFAULTS {
        let mut result = Ok(Output::Plugin(host.install_bundled(name)?));
        let mut event = finish(
            &transaction,
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
            save(&transaction, &info, true)?;
            event = Some(Event::PluginChanged(info.summary));
        }
        if let Some(event) = event {
            transaction
                .execute("INSERT INTO events(body) VALUES(?1)", [encode(&event)?])
                .map_err(storage_error)?;
        }
    }
    transaction
        .execute("UPDATE node SET plugin_defaults=1 WHERE singleton=1", [])
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}
