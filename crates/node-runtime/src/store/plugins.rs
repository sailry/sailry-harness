//! Plugin inventory shares durable admission, configuration transactions and Node events.
mod actions;
pub(super) mod authorization;
mod catalog;
pub(super) mod collection;
pub(in crate::store) mod command;
pub(in crate::store) mod connections;
pub(super) mod distribution;
pub(in crate::store) mod grants;
mod http;
pub(super) mod invocations;
pub(in crate::store) mod mcp;
pub(super) mod models;
mod repository;
mod script;
pub(super) mod settings;
mod skills;
pub(in crate::store) mod storage;
pub(super) mod transaction;
pub(super) mod updates;
pub(super) mod views;
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{
    Command, ErrorCode, Event, Fault, Output,
    plugin::{self, Info, Summary},
};

use super::database::{encode, storage_error};

pub(super) fn get(db: &Connection, name: &str) -> Result<Option<Info>, Fault> {
    crate::plugins::validate_name(name)?;
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM plugins WHERE name=?1 AND installed=1",
            [name],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let stored: Option<Info> = body
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?;
    Ok(stored)
}

pub(super) fn list(db: &Connection) -> Result<Vec<Summary>, Fault> {
    let mut query = db
        .prepare("SELECT body FROM plugins WHERE installed=1 ORDER BY name")
        .map_err(storage_error)?;
    let mut entries: Vec<Summary> = query
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
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

fn next(db: &Connection, name: &str) -> Result<u64, Fault> {
    let previous: Option<i64> = db
        .query_row(
            "SELECT revision FROM plugins WHERE name=?1",
            [name],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    previous
        .unwrap_or(0)
        .checked_add(1)
        .map(|value| value as u64)
        .ok_or_else(|| Fault::new(ErrorCode::Conflict, "plugin revision exhausted"))
}

fn save(db: &Connection, info: &Info, installed: bool) -> Result<(), Fault> {
    db.execute("INSERT INTO plugins(name,revision,installed,body) VALUES(?1,?2,?3,?4) ON CONFLICT(name) DO UPDATE SET revision=excluded.revision,installed=excluded.installed,body=excluded.body",
        params![info.summary.name, info.summary.revision as i64, installed, encode(info)?]).map_err(storage_error)?;
    Ok(())
}

pub(super) fn check_install(
    db: &Connection,
    name: &str,
    expected: u64,
) -> Result<Option<Info>, Fault> {
    let current = get(db, name)?;
    super::commands::check_revision(
        current.as_ref().map_or(0, |info| info.summary.revision),
        expected,
    )?;
    if current.is_none() && list(db)?.len() >= plugin::MAX_INSTALLED {
        return Err(Fault::new(ErrorCode::Busy, "plugin inventory is full"));
    }
    Ok(current)
}

pub(super) fn finish(
    db: &Connection,
    command: &Command,
    result: &mut sailry_link::Response,
) -> Result<Option<Event>, Fault> {
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
            .map_err(storage_error)?;
        return match mcp::save(db, package, configuration) {
            Ok(info) => {
                db.execute_batch("RELEASE mcp_configuration")
                    .map_err(storage_error)?;
                let event = Event::PluginChanged(info.summary.clone());
                *result = Ok(Output::Plugin(info));
                Ok(Some(event))
            }
            Err(error) => {
                db.execute_batch("ROLLBACK TO mcp_configuration; RELEASE mcp_configuration")
                    .map_err(storage_error)?;
                *result = Err(error);
                Ok(None)
            }
        };
    }
    if matches!(command, Command::InstallMcp { .. }) {
        db.execute_batch("SAVEPOINT mcp_install")
            .map_err(storage_error)?;
        return match finish_package(db, command, result) {
            Ok(event) => {
                db.execute_batch("RELEASE mcp_install")
                    .map_err(storage_error)?;
                Ok(event)
            }
            Err(error) => {
                db.execute_batch("ROLLBACK TO mcp_install; RELEASE mcp_install")
                    .map_err(storage_error)?;
                *result = Err(error);
                Ok(None)
            }
        };
    }
    finish_package(db, command, result)
}

fn finish_package(
    db: &Connection,
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
    let current = match check_install(db, name, *expected_revision) {
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
        params![info.summary.digest, name, encode(info)?],
    )
    .map_err(storage_error)?;
    info.origin = match command {
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
    info.summary.revision = next(db, name)?;
    info.summary.settings_revision = current
        .as_ref()
        .map_or(0, |info| info.summary.settings_revision);
    info.summary.enabled = current
        .as_ref()
        .is_none_or(|current| current.summary.enabled);
    save(db, info, true)?;
    super::agent::continuation::invalidate(db, name)?;
    if let Command::InstallMcp {
        definition,
        secrets,
        ..
    } = command
    {
        let updates = crate::plugins::standalone_mcp::updates(definition, secrets)?;
        settings::save(db, &info.summary.reference(), &Default::default(), &updates)?;
        *info = required(db, name)?;
    }
    Ok(Some(Event::PluginChanged(info.summary.clone())))
}

pub(super) fn execute(
    db: &Connection,
    command: &Command,
) -> Result<(Output, Option<Event>), Fault> {
    match command {
        Command::ListPluginModels => Ok((Output::PluginModels(models::catalog(db)?), None)),
        Command::ResolvePluginModel {
            model,
            effort,
            config,
        } => Ok((
            Output::SessionConfig(models::session(db, model, *effort, config.as_ref())?),
            None,
        )),
        Command::ReadMcpAuthorization { package, server } => Ok((
            Output::McpAuthorization(authorization::read(db, package, server)?),
            None,
        )),
        Command::RevokeMcpAuthorization { package, server } => {
            let info = authorization::revoke(db, package, server)?;
            Ok((
                Output::McpAuthorization(authorization::read(
                    db,
                    &info.summary.reference(),
                    server,
                )?),
                Some(Event::PluginChanged(info.summary)),
            ))
        }
        Command::ReadPluginSettings { package } => {
            Ok((Output::PluginSettings(settings::read(db, package)?), None))
        }
        Command::ReadPluginSecret { package, field } => Ok((
            Output::PluginSecret(settings::read_secret(db, package, field)?),
            None,
        )),
        Command::SavePluginSettings {
            package,
            values,
            secrets,
        } => {
            let state = settings::save(db, package, values, secrets)?;
            let info = required(db, &package.name)?;
            Ok((
                Output::PluginSettings(state),
                Some(Event::PluginChanged(info.summary)),
            ))
        }
        Command::ListPlugins => Ok((Output::Plugins(list(db)?), None)),
        Command::ReadPlugin { name } => Ok((Output::Plugin(required(db, name)?), None)),
        Command::ReadPluginVersion { package } => {
            let mut packages =
                crate::store::agent::plugins::packages(db, std::slice::from_ref(package))?;
            Ok((Output::Plugin(packages.remove(0)), None))
        }
        Command::SetPluginEnabled {
            name,
            expected_revision,
            enabled,
        } => {
            let mut info = required(db, name)?;
            super::commands::check_revision(info.summary.revision, *expected_revision)?;
            info.summary.revision = next(db, name)?;
            info.summary.enabled = *enabled;
            save(db, &info, true)?;
            if !enabled {
                super::agent::continuation::invalidate(db, name)?;
            }
            Ok((
                Output::Plugin(info.clone()),
                Some(Event::PluginChanged(info.summary)),
            ))
        }
        Command::RemovePlugin {
            name,
            expected_revision,
        } => {
            let mut info = required(db, name)?;
            super::commands::check_revision(info.summary.revision, *expected_revision)?;
            info.summary.revision = next(db, name)?;
            info.summary.enabled = false;
            save(db, &info, false)?;
            super::agent::continuation::invalidate(db, name)?;
            Ok((
                Output::Plugins(list(db)?),
                Some(Event::PluginRemoved { name: name.clone() }),
            ))
        }
        _ => Err(storage_error("plugin command expected")),
    }
}

pub(super) fn execute_storage(
    db: &Connection,
    request: &sailry_protocol::Request,
) -> Result<(Output, Option<Event>), Fault> {
    let output = storage::execute(db, request)?;
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

fn required(db: &Connection, name: &str) -> Result<Info, Fault> {
    get(db, name)?.ok_or_else(|| Fault::new(ErrorCode::NotFound, "plugin is not installed"))
}
pub(super) mod execution;

#[cfg(test)]
mod tests;
