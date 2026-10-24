//! Plugin inventory shares durable admission, configuration transactions and Node events.
mod actions;
pub(in crate::store) mod cards;
mod catalog;
pub(super) mod collection;
pub(in crate::store) mod command;
pub(in crate::store) mod connection;
pub(in crate::store) mod connections;
pub(in crate::store) mod connectors;
pub(super) mod distribution;
mod http;
pub(in crate::store) mod installing;
pub(super) mod invocations;
pub(in crate::store) mod mcp;
pub(super) mod models;
pub(in crate::store) mod options;
pub(in crate::store) mod policy;
mod repository;
mod script;
pub(super) mod services;
pub(super) mod settings;
mod skills;
pub(in crate::store) mod storage;
pub(super) mod transaction;
pub(super) mod updates;
pub(super) mod views;
use sailry_protocol::{
    Command, ErrorCode, Event, Fault, Output,
    plugin::{self, Info, Summary},
};
use turso::{Connection, params};

use super::database::{encode, storage_error};

pub(super) async fn get(db: &Connection, name: &str) -> Result<Option<Info>, Fault> {
    crate::plugins::validate_name(name)?;
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM plugins WHERE name=?1 AND installed=1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([name]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    let stored: Option<Info> = body
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?;
    Ok(stored)
}

pub(super) async fn list(db: &Connection) -> Result<Vec<Summary>, Fault> {
    let mut query = db
        .prepare("SELECT body FROM plugins WHERE installed=1 ORDER BY name")
        .await
        .map_err(storage_error)?;
    let mut entries: Vec<Summary> = (async {
        let mut rows = query.query(()).await?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await? {
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<Vec<u8>>(0))(&row);
            values.push(mapped);
        }
        Ok::<_, turso::Error>(values.into_iter())
    })
    .await
    .map_err(storage_error)?
    .map(|body| {
        serde_json::from_slice::<Info>(&body.map_err(storage_error)?)
            .map(|info| info.summary)
            .map_err(storage_error)
    })
    .collect::<Result<_, Fault>>()?;
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

async fn next(db: &Connection, name: &str) -> Result<u64, Fault> {
    let previous: Option<i64> = {
        let mut statement = db
            .prepare("SELECT revision FROM plugins WHERE name=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([name]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    previous
        .unwrap_or(0)
        .checked_add(1)
        .map(|value| value as u64)
        .ok_or_else(|| Fault::new(ErrorCode::Conflict, "plugin revision exhausted"))
}

async fn save(db: &Connection, info: &Info, installed: bool) -> Result<(), Fault> {
    db.execute("INSERT INTO plugins(name,revision,installed,body) VALUES(?1,?2,?3,?4) ON CONFLICT(name) DO UPDATE SET revision=excluded.revision,installed=excluded.installed,body=excluded.body",
        params![info.summary.name.as_str(), info.summary.revision as i64, installed, encode(info)?]).await.map_err(storage_error)?;
    Ok(())
}

async fn enable(db: &Connection, mut info: Info, enabled: bool) -> Result<Info, Fault> {
    let name = &info.summary.name;
    info.summary.revision = next(db, name).await?;
    info.summary.enabled = enabled;
    save(db, &info, true).await?;
    if !enabled {
        super::agent::continuation::invalidate(db, name).await?;
    }
    Ok(info)
}

pub(super) async fn synchronize(
    db: &Connection,
    name: &str,
    enabled: bool,
) -> Result<Option<Summary>, Fault> {
    let Some(info) = get(db, name).await? else {
        if enabled && {
            let mut statement = db
                .prepare("SELECT EXISTS(SELECT 1 FROM plugins WHERE name=?1 AND installed=0)")
                .await
                .map_err(storage_error)?;
            let row = statement.query_row([name]).await.map_err(storage_error)?;
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<bool>(0))(&row);
            mapped.map_err(storage_error)?
        } {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "plugin must be installed before enabling it",
            ));
        }
        return Ok(None);
    };
    if info.summary.enabled == enabled {
        return Ok(None);
    }
    Ok(Some(enable(db, info, enabled).await?.summary))
}

pub(super) async fn check_install(
    db: &Connection,
    name: &str,
    expected: u64,
) -> Result<Option<Info>, Fault> {
    let current = get(db, name).await?;
    super::commands::check_revision(
        current.as_ref().map_or(0, |info| info.summary.revision),
        expected,
    )?;
    if current.is_none() && list(db).await?.len() >= plugin::MAX_INSTALLED {
        return Err(Fault::new(ErrorCode::Busy, "plugin inventory is full"));
    }
    Ok(current)
}

pub(super) async fn prepare_enable(
    db: &Connection,
    name: &str,
    expected: u64,
) -> Result<Info, Fault> {
    let info = required(db, name).await?;
    super::commands::check_revision(info.summary.revision, expected)?;
    Ok(info)
}

pub(super) async fn retain_python(
    db: &Connection,
    host: &crate::plugins::Host,
) -> Result<(), Fault> {
    let mut packages = Vec::new();
    for summary in list(db).await? {
        packages.push(required(db, &summary.name).await?);
    }
    host.retain_python(packages);
    Ok(())
}

pub(super) async fn finish(
    db: &Connection,
    profile: Option<&std::path::Path>,
    command: &Command,
    result: &mut sailry_link::Response,
) -> Result<Option<Event>, Fault> {
    if let Command::SetPluginEnabled {
        name,
        expected_revision,
        enabled: true,
    } = command
    {
        if result.is_err() {
            return Ok(None);
        }
        let Ok(Output::Plugin(prepared)) = result else {
            return Err(storage_error("prepared plugin identity expected"));
        };
        let current = match prepare_enable(db, name, *expected_revision).await {
            Ok(info) => info,
            Err(error) => {
                *result = Err(error);
                return Ok(None);
            }
        };
        if prepared.summary.reference() != current.summary.reference() {
            return Err(storage_error("prepared plugin identity changed"));
        }
        let info = enable(db, current, true).await?;
        services::apply(db, &info, true).await?;
        let event = Event::PluginChanged(info.summary.clone());
        *result = Ok(Output::Plugin(mcp::project(db, profile, info).await?));
        return Ok(Some(event));
    }
    if let Command::SavePluginMcp {
        package,
        configuration,
    } = command
    {
        if result.is_err() {
            return Ok(None);
        }
        let Ok(Output::PluginMcp(state)) = result else {
            return Err(storage_error(
                "MCP configuration inspection result expected",
            ));
        };
        if state.package != *package || state.configuration != *configuration {
            return Err(storage_error("MCP configuration inspection mismatch"));
        }
        db.execute_batch("SAVEPOINT mcp_configuration")
            .await
            .map_err(storage_error)?;
        return match mcp::save(db, profile, package, configuration).await {
            Ok(info) => {
                db.execute_batch("RELEASE mcp_configuration")
                    .await
                    .map_err(storage_error)?;
                let event = Event::PluginChanged(info.summary.clone());
                *result = Ok(Output::Plugin(mcp::project(db, profile, info).await?));
                Ok(Some(event))
            }
            Err(error) => {
                db.execute_batch("ROLLBACK TO mcp_configuration; RELEASE mcp_configuration")
                    .await
                    .map_err(storage_error)?;
                *result = Err(error);
                Ok(None)
            }
        };
    }
    if matches!(command, Command::InstallMcp { .. }) {
        db.execute_batch("SAVEPOINT mcp_install")
            .await
            .map_err(storage_error)?;
        return match finish_package(db, profile, command, result).await {
            Ok(event) => {
                db.execute_batch("RELEASE mcp_install")
                    .await
                    .map_err(storage_error)?;
                Ok(event)
            }
            Err(error) => {
                db.execute_batch("ROLLBACK TO mcp_install; RELEASE mcp_install")
                    .await
                    .map_err(storage_error)?;
                *result = Err(error);
                Ok(None)
            }
        };
    }
    finish_package(db, profile, command, result).await
}

async fn finish_package(
    db: &Connection,
    profile: Option<&std::path::Path>,
    command: &Command,
    result: &mut sailry_link::Response,
) -> Result<Option<Event>, Fault> {
    let (Command::InstallPlugin {
        name,
        expected_revision,
        ..
    }
    | Command::InstallPluginUpload {
        name,
        expected_revision,
        ..
    }
    | Command::InstallPluginSource {
        name,
        expected_revision,
        ..
    }
    | Command::InstallBundledPlugin {
        name,
        expected_revision,
    }
    | Command::InstallSkillUpload {
        name,
        expected_revision,
        ..
    }
    | Command::InstallRegistrySkill {
        name,
        expected_revision,
        ..
    }
    | Command::InstallSkill {
        name,
        expected_revision,
        ..
    }
    | Command::InstallMcp {
        name,
        expected_revision,
        ..
    }) = command
    else {
        return Ok(None);
    };
    let Ok(Output::Plugin(info)) = result else {
        return Ok(None);
    };
    let current = match check_install(db, name, *expected_revision).await {
        Ok(current) => current,
        Err(error) => {
            *result = Err(error);
            return Ok(None);
        }
    };
    if info.summary.name != *name {
        return Err(storage_error("installed plugin identity mismatch"));
    }
    info.origin = None;
    db.execute(
        // Explicit reinstallation refreshes parsed metadata for the same immutable bytes.
        "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3) ON CONFLICT(digest) DO UPDATE SET body=excluded.body WHERE name=excluded.name",
        params![info.summary.digest.as_str(), name.as_str(), encode(info)?],
    ).await
    .map_err(storage_error)?;
    info.origin = match command {
        Command::InstallSkillUpload { source, .. } => Some(match source.kind {
            plugin::UploadSource::Directory => plugin::Origin::Directory,
            plugin::UploadSource::Archive => plugin::Origin::Archive,
        }),
        Command::InstallRegistrySkill { source, .. } => Some(plugin::Origin::Registry {
            source: source.clone(),
        }),
        Command::InstallBundledPlugin { .. } => Some(plugin::Origin::Bundled),
        Command::InstallPluginUpload { source, .. } => Some(match source {
            plugin::UploadSource::Directory => plugin::Origin::Directory,
            plugin::UploadSource::Archive => plugin::Origin::Archive,
        }),
        Command::InstallPlugin { worktree, path, .. } => Some(plugin::Origin::Worktree {
            worktree: *worktree,
            path: path.clone(),
        }),
        Command::InstallPluginSource { source, path, .. }
        | Command::InstallSkill { source, path, .. } => Some(plugin::Origin::Online {
            source: source.clone(),
            path: path.clone(),
        }),
        _ => None,
    };
    info.summary.revision = next(db, name).await?;
    info.summary.settings_revision = current
        .as_ref()
        .map_or(0, |info| info.summary.settings_revision);
    info.summary.enabled = match current.as_ref() {
        Some(current) => current.summary.enabled,
        None if info.summary.revision == 1 => services::initial(db, info).await?.unwrap_or(true),
        None => true,
    };
    services::check(db, info).await?;
    policy::check(db, info).await?;
    options::check(db, info).await?;
    if let Some(previous) = &current
        && previous
            .extension
            .as_ref()
            .and_then(|extension| extension.enabled_service)
            != info
                .extension
                .as_ref()
                .and_then(|extension| extension.enabled_service)
    {
        services::apply(db, previous, false).await?;
    }
    save(db, info, true).await?;
    if connectors::migrate(db, profile, info).await? {
        super::database::validation::stored(db)
            .await
            .map_err(storage_error)?;
    }
    policy::migrate(db, info).await?;
    services::apply(db, info, info.summary.enabled).await?;
    super::agent::continuation::invalidate(db, name).await?;
    if let Command::InstallMcp {
        definition,
        secrets,
        ..
    } = command
    {
        let updates = crate::plugins::standalone_mcp::updates(definition, secrets)?;
        settings::save(db, &info.summary.reference(), &Default::default(), &updates).await?;
        *info = required(db, name).await?;
    }
    *info = mcp::project(db, profile, info.clone()).await?;
    Ok(Some(Event::PluginChanged(info.summary.clone())))
}

pub(super) fn execute<'a>(
    db: &'a Connection,
    profile: Option<&'a std::path::Path>,
    command: &'a Command,
) -> sailry_link::Pending<'a, Result<(Output, Option<Event>), Fault>> {
    match command {
        Command::ListPluginModels => {
            Box::pin(async move { Ok((Output::PluginModels(models::catalog(db).await?), None)) })
        }
        Command::ResolvePluginModel {
            model,
            effort,
            config,
        } => Box::pin(async move {
            Ok((
                Output::SessionConfig(models::session(db, model, *effort, config.as_ref()).await?),
                None,
            ))
        }),
        Command::ReadPluginSettings { package } => Box::pin(async move {
            Ok((
                Output::PluginSettings(settings::read(db, package).await?),
                None,
            ))
        }),
        Command::ReadPluginSecret { package, field } => Box::pin(async move {
            Ok((
                Output::PluginSecret(settings::read_secret(db, package, field).await?),
                None,
            ))
        }),
        Command::SavePluginSettings {
            package,
            values,
            secrets,
        } => Box::pin(async move {
            let state = settings::save(db, package, values, secrets).await?;
            let info = required(db, &package.name).await?;
            Ok((
                Output::PluginSettings(state),
                Some(Event::PluginChanged(info.summary)),
            ))
        }),
        Command::ListPlugins => {
            Box::pin(async move { Ok((Output::Plugins(list(db).await?), None)) })
        }
        Command::ReadPlugin { name } => Box::pin(async move {
            Ok((
                Output::Plugin(mcp::project(db, profile, required(db, name).await?).await?),
                None,
            ))
        }),
        Command::ReadPluginVersion { package } => Box::pin(async move {
            let mut packages =
                crate::store::agent::plugins::packages(db, std::slice::from_ref(package)).await?;
            Ok((
                Output::Plugin(mcp::project(db, profile, packages.remove(0)).await?),
                None,
            ))
        }),
        Command::SetPluginMcpAllowed {
            package,
            server,
            allowed,
        } => Box::pin(async move {
            let info = mcp::set_allowed(db, profile, package, server, *allowed).await?;
            let event = Event::PluginChanged(info.summary.clone());
            Ok((
                Output::Plugin(mcp::project(db, profile, info).await?),
                Some(event),
            ))
        }),
        Command::SetPluginEnabled {
            name,
            expected_revision,
            enabled,
        } => Box::pin(async move {
            let info = required(db, name).await?;
            super::commands::check_revision(info.summary.revision, *expected_revision)?;
            let info = enable(db, info, *enabled).await?;
            services::apply(db, &info, *enabled).await?;
            Ok((
                Output::Plugin(mcp::project(db, profile, info.clone()).await?),
                Some(Event::PluginChanged(info.summary)),
            ))
        }),
        Command::RemovePlugin {
            name,
            expected_revision,
        } => Box::pin(async move {
            let mut info = required(db, name).await?;
            super::commands::check_revision(info.summary.revision, *expected_revision)?;
            info.summary.revision = next(db, name).await?;
            info.summary.enabled = false;
            connectors::remove_mcp(db, &info).await?;
            save(db, &info, false).await?;
            services::apply(db, &info, false).await?;
            super::agent::continuation::invalidate(db, name).await?;
            Ok((
                Output::Plugins(list(db).await?),
                Some(Event::PluginRemoved { name: name.clone() }),
            ))
        }),
        _ => Box::pin(async move { Err(storage_error("plugin command expected")) }),
    }
}

pub(super) async fn execute_storage(
    db: &Connection,
    request: &sailry_protocol::Request,
) -> Result<(Output, Option<Event>), Fault> {
    let output = storage::execute(db, request).await?;
    let changed = matches!(
        request.command,
        Command::WritePluginValue { .. }
            | Command::WriteIndexedPluginValue { .. }
            | Command::WritePluginConversationValue { .. }
    ) || matches!((&request.command,&output),(Command::RemovePluginValue { .. },Output::PluginValue(entry)) if entry.revision>0)
        || matches!((&request.command,&output),(Command::RemovePluginConversationValue { .. },Output::PluginConversationValue(entry)) if entry.revision>0);
    let event = changed.then(|| Event::PluginValuesChanged {
        name: request
            .plugin
            .as_ref()
            .expect("validated storage provenance")
            .package
            .name
            .clone(),
    });
    Ok((output, event))
}

async fn required(db: &Connection, name: &str) -> Result<Info, Fault> {
    get(db, name)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "plugin is not installed"))
}
pub(super) mod execution;

#[cfg(test)]
mod tests;
