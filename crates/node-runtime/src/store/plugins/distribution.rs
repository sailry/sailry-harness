//! Missing defaults and bundled settings refresh reuse ordinary installation.
use super::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(in crate::store) async fn install(
    db: &mut Connection,
    host: &crate::plugins::Host,
) -> Result<(), Fault> {
    let transaction = db.transaction().await.map_err(storage_error)?;
    if let Err(error) = initialize(&transaction, host).await {
        transaction.rollback().await.map_err(storage_error)?;
        return Err(error);
    }
    transaction.commit().await.map_err(storage_error)
}

pub(in crate::store) async fn initialize(
    db: &Connection,
    host: &crate::plugins::Host,
) -> Result<(), Fault> {
    for &(name, enabled) in crate::plugins::DEFAULTS {
        let recorded: bool = {
            let mut statement = db
                .prepare("SELECT EXISTS(SELECT 1 FROM plugins WHERE name=?1)")
                .await
                .map_err(storage_error)?;
            let row = statement.query_row([name]).await.map_err(storage_error)?;
            let mapped: turso::Result<_> = row.get(0);
            mapped.map_err(storage_error)?
        };
        if recorded {
            continue;
        }
        let info = host.install_bundled(name)?;
        let enabled = services::initial(db, &info).await?.unwrap_or(enabled);
        let mut result = Ok(Output::Plugin(info));
        let mut event = finish(
            db,
            host.profile(),
            &Command::InstallBundledPlugin {
                name: name.into(),
                expected_revision: 0,
            },
            &mut result,
        )
        .await?;
        if let Output::Plugin(mut info) = result?
            && info.summary.enabled != enabled
        {
            info.summary.enabled = enabled;
            save(db, &info, true).await?;
            event = Some(Event::PluginChanged(info.summary));
        }
        if let Some(event) = event {
            db.execute(
                "INSERT INTO events(cursor,body) VALUES(?1,?2)",
                params![
                    crate::store::database::counters::next(
                        db,
                        crate::store::database::counters::Namespace::Events
                    )
                    .await? as i64,
                    encode(&event)?
                ],
            )
            .await
            .map_err(storage_error)?;
        }
    }
    refresh(db, host).await?;
    for summary in list(db).await? {
        let mut info = required(db, &summary.name).await?;
        super::connectors::migrate(db, host.profile(), &mut info).await?;
    }
    crate::store::providers::authentication::initialize(db).await?;
    db.execute("UPDATE node SET plugin_defaults=1 WHERE singleton=1", ())
        .await
        .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) async fn needs_connections(
    db: &Connection,
    host: &crate::plugins::Host,
) -> Result<bool, Fault> {
    for summary in list(db).await? {
        let info = required(db, &summary.name).await?;
        if super::connectors::needs_migration(db, &info).await? {
            return Ok(true);
        }
        if info.origin == Some(plugin::Origin::Bundled)
            && crate::plugins::Host::bundled_newer(&summary.name, summary.version.as_deref())
                .unwrap_or(false)
        {
            let candidate = host.install_bundled(&summary.name)?;
            if super::connectors::needs_migration(db, &candidate).await? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

async fn refresh(db: &Connection, host: &crate::plugins::Host) -> Result<(), Fault> {
    for summary in list(db).await? {
        let name = summary.name.as_str();
        let Some(current) = get(db, name).await? else {
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
            host.profile(),
            &Command::InstallBundledPlugin {
                name: name.into(),
                expected_revision: current.summary.revision,
            },
            &mut result,
        )
        .await?;
        result?;
        if let Some(event) = event {
            db.execute(
                "INSERT INTO events(cursor,body) VALUES(?1,?2)",
                params![
                    crate::store::database::counters::next(
                        db,
                        crate::store::database::counters::Namespace::Events
                    )
                    .await? as i64,
                    encode(&event)?
                ],
            )
            .await
            .map_err(storage_error)?;
        }
    }
    Ok(())
}
