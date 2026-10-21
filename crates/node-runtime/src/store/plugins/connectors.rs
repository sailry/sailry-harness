//! Connection ownership is independent of package settings and model accounts.
use super::*;
use sailry_protocol::{
    RequestId, Secret,
    plugin::connector::{
        Auth, Connection as Projection, Declaration, Id, Identity, Injection, Input, Kind, State,
        Type, identifier,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
mod consent;
pub(in crate::store) mod mcp;
mod registry;
pub(in crate::store) use registry::declarations;
pub(in crate::store) mod watch;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Record {
    pub id: Id,
    pub plugin: String,
    pub connector: String,
    pub revision: u64,
    pub method: Kind,
    pub enabled: bool,
    pub fields: BTreeMap<String, String>,
    pub secrets: BTreeMap<String, Secret>,
    pub grant: Option<Grant>,
    #[serde(default = "initial_revision")]
    pub grant_revision: u64,
    pub identity: Option<Identity>,
    pub bound: Option<Bound>,
    pub state: State,
    pub checked_at_ms: Option<u64>,
}
fn initial_revision() -> u64 {
    1
}

#[derive(Clone, Serialize, Deserialize)]
pub(in crate::store) struct Grant {
    pub access: Secret,
    pub refresh: Option<Secret>,
    pub expires_at_ms: Option<u64>,
    #[serde(default)]
    pub extra: BTreeMap<String, Secret>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::store) struct Bound {
    pub origin: Option<plugin::Origin>,
    pub auth: Auth,
    pub origins: Vec<String>,
    pub inject: Vec<Injection>,
    #[serde(default)]
    pub targets: BTreeMap<String, plugin::mcp::Definition>,
}

pub(in crate::store) fn types(db: &Connection) -> Result<Vec<Type>, Fault> {
    let mut result = Vec::new();
    for summary in list(db)?.into_iter().filter(|summary| summary.enabled) {
        let info = required(db, &summary.name)?;
        for declaration in registry::declarations(&info) {
            result.push(Type {
                package: summary.reference(),
                declaration,
            });
        }
    }
    Ok(result)
}

pub(in crate::store) fn info(db: &Connection, name: &str) -> Result<Info, Fault> {
    required(db, name)
}

pub(in crate::store) fn declaration(
    db: &Connection,
    package: &plugin::Reference,
    connector: &str,
) -> Result<(Info, Declaration), Fault> {
    let info = settings::current(db, package)?;
    if !info.summary.enabled {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "connector package is disabled",
        ));
    }
    let declaration = registry::declarations(&info)
        .into_iter()
        .find(|item| item.id == connector)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "connector is unavailable"))?;
    Ok((info, declaration))
}

pub(in crate::store) fn bound(
    db: &Connection,
    profile: Option<&std::path::Path>,
    info: &Info,
    declaration: &Declaration,
    method: Kind,
) -> Result<Option<Bound>, Fault> {
    let Some(auth) = declaration.auth.iter().find(|auth| auth.kind() == method) else {
        return Ok(None);
    };
    if !auth.consent() {
        return Ok(None);
    }
    let mut origins = declaration.origins.clone();
    origins.sort();
    let mut auth = auth.clone();
    let scopes = match &mut auth {
        Auth::Oauth2 { scopes, .. } => Some(scopes),
        Auth::Application {
            registration: Some(registration),
            ..
        } => Some(&mut registration.scopes),
        _ => None,
    };
    if let Some(scopes) = scopes {
        scopes.sort();
    }
    Ok(Some(Bound {
        origin: info.origin.clone(),
        auth,
        origins,
        inject: declaration.inject.clone(),
        targets: targets(db, profile, info, declaration)?,
    }))
}

fn targets(
    db: &Connection,
    profile: Option<&std::path::Path>,
    info: &Info,
    declaration: &Declaration,
) -> Result<BTreeMap<String, plugin::mcp::Definition>, Fault> {
    let servers = declaration
        .inject
        .iter()
        .filter_map(|injection| injection.server.as_ref())
        .collect::<std::collections::BTreeSet<_>>();
    if servers.is_empty() {
        return Ok(BTreeMap::new());
    }
    let configuration = match settings::stored(db, info)?.mcp {
        Some(configuration) => configuration,
        None => crate::plugins::Host::new(profile.map(std::path::Path::to_owned))
            .mcp_configuration(
                &info.summary.reference(),
                sailry_link::CancellationToken::new(),
            )?,
    };
    servers
        .into_iter()
        .map(|name| {
            let server = configuration
                .servers
                .get(name)
                .ok_or_else(|| Fault::invalid("connector MCP server is unavailable"))?;
            let target = match server {
                plugin::mcp::Server::Http { url, headers }
                | plugin::mcp::Server::Sse { url, headers } => {
                    let url = url::Url::parse(url)
                        .map_err(|_| Fault::invalid("invalid connector MCP URL"))?;
                    if !registry::endpoint(info, declaration, url.as_str()) {
                        return Err(Fault::new(
                            ErrorCode::PermissionDenied,
                            "connector MCP server is outside its origins",
                        ));
                    }
                    let headers = headers.keys().cloned().collect();
                    if matches!(server, plugin::mcp::Server::Sse { .. }) {
                        plugin::mcp::Definition::Sse {
                            url: url.into(),
                            headers,
                        }
                    } else {
                        plugin::mcp::Definition::StreamableHttp {
                            url: url.into(),
                            headers,
                        }
                    }
                }
                plugin::mcp::Server::Stdio {
                    command, args, env, ..
                } => plugin::mcp::Definition::Stdio {
                    command: command.clone(),
                    args: args.clone(),
                    env: env.keys().cloned().collect(),
                },
            };
            Ok((name.clone(), target))
        })
        .collect()
}

pub(in crate::store) fn records(db: &Connection) -> Result<Vec<Record>, Fault> {
    let mut query = db
        .prepare("SELECT body FROM plugin_connections ORDER BY plugin,connector,id")
        .map_err(storage_error)?;
    query
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
        .map(|bytes| serde_json::from_slice(&bytes.map_err(storage_error)?).map_err(storage_error))
        .collect()
}

pub(in crate::store) fn read(db: &Connection, id: Id) -> Result<Record, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM plugin_connections WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    serde_json::from_slice(
        &body.ok_or_else(|| Fault::new(ErrorCode::NotFound, "connection is unavailable"))?,
    )
    .map_err(storage_error)
}

pub(in crate::store) fn save(db: &Connection, record: &Record) -> Result<(), Fault> {
    validate(record)?;
    db.execute("INSERT INTO plugin_connections(id,plugin,connector,revision,body) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,body=excluded.body WHERE plugin=excluded.plugin AND connector=excluded.connector", params![record.id.to_string(),record.plugin,record.connector,record.revision as i64,encode(record)?]).map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) fn validate(record: &Record) -> Result<(), Fault> {
    if record.revision == 0
        || record.revision > i64::MAX as u64
        || !identifier(&record.connector)
        || crate::plugins::validate_name(&record.plugin).is_err()
        || record.grant_revision == 0
        || record.grant_revision > i64::MAX as u64
        || record.fields.len() > 8
        || record.secrets.len() > 8
        || (record.method == Kind::LocalCli
            && (record.grant.is_some() || !record.secrets.is_empty()))
    {
        return Err(Fault::invalid("invalid stored connection"));
    }
    Ok(())
}

pub(in crate::store) fn project(
    db: &Connection,
    profile: Option<&std::path::Path>,
    record: &Record,
) -> Result<Projection, Fault> {
    let info = get(db, &record.plugin)?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "connector package is unavailable"))?;
    let declaration = registry::declarations(&info)
        .into_iter()
        .find(|item| item.id == record.connector);
    let current = declaration
        .as_ref()
        .map(|declaration| bound(db, profile, &info, declaration, record.method))
        .transpose()?
        .flatten();
    let consent = consent::changes(record.bound.as_ref(), current.as_ref());
    let state = match declaration {
        None => State::NotConnected,
        Some(_) if !info.summary.enabled => State::NotConnected,
        Some(_) if consent.is_some() => State::NeedsConfirmation,
        Some(declaration)
            if !declaration
                .auth
                .iter()
                .any(|auth| auth.kind() == record.method) =>
        {
            State::NotConnected
        }
        _ => record.state,
    };
    Ok(Projection {
        id: record.id,
        package: info.summary.reference(),
        connector: record.connector.clone(),
        method: record.method,
        revision: record.revision,
        enabled: record.enabled,
        state,
        identity: record.identity.clone(),
        expires_at_ms: record.grant.as_ref().and_then(|grant| grant.expires_at_ms),
        checked_at_ms: record.checked_at_ms,
        fields: record.fields.clone(),
        configured: record.secrets.keys().cloned().collect(),
        consent,
    })
}

pub(in crate::store) fn connections(
    db: &Connection,
    profile: Option<&std::path::Path>,
) -> Result<Vec<Projection>, Fault> {
    let types = types(db)?;
    let mut output = Vec::new();
    for record in records(db)? {
        if types.iter().any(|kind| {
            kind.package.name == record.plugin && kind.declaration.id == record.connector
        }) {
            output.push(project(db, profile, &record)?);
        }
    }
    Ok(output)
}

pub(in crate::store) fn input(
    record: &mut Record,
    declaration: &Declaration,
    input: &Input,
) -> Result<(), Fault> {
    for (name, value) in &input.fields {
        if value.len() > 16 * 1024
            || !declaration
                .fields
                .iter()
                .any(|field| &field.name == name && !field.secret)
        {
            return Err(Fault::invalid("invalid connector field"));
        }
        record.fields.insert(name.clone(), value.clone());
    }
    for (name, value) in &input.secrets {
        if value.expose().len() > 64 * 1024
            || !declaration
                .fields
                .iter()
                .any(|field| &field.name == name && field.secret)
        {
            return Err(Fault::invalid("invalid connector secret"));
        }
        if !value.expose().is_empty() {
            record.secrets.insert(name.clone(), value.clone());
        }
    }
    for field in &declaration.fields {
        if !field.secret
            && !record.fields.contains_key(&field.name)
            && let Some(value) = &field.default
        {
            record.fields.insert(field.name.clone(), value.clone());
        }
    }
    Ok(())
}

pub(in crate::store) fn create(
    db: &Connection,
    profile: Option<&std::path::Path>,
    command: &Command,
) -> Result<(Record, Declaration), Fault> {
    let Command::StartConnectorLogin {
        package,
        connector,
        connection: id,
        expected_revision: expected,
        method,
        input: values,
    } = command
    else {
        return Err(Fault::invalid("connector login expected"));
    };
    let (id, expected, method) = (*id, *expected, *method);
    let (info, declaration) = declaration(db, package, connector)?;
    if !declaration.auth.iter().any(|auth| auth.kind() == method) {
        return Err(Fault::invalid(
            "connector authentication method is unavailable",
        ));
    }
    let mut record = if let Some(id) = id {
        let record = read(db, id)?;
        if record.plugin != package.name || record.connector != *connector {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "connection belongs to another connector",
            ));
        }
        super::super::commands::check_revision(
            record.revision,
            expected.ok_or_else(|| Fault::invalid("connection revision is required"))?,
        )?;
        record
    } else {
        if records(db)?
            .iter()
            .any(|record| record.plugin == package.name && record.connector == *connector)
        {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "connector already has a connection",
            ));
        }
        Record {
            id: RequestId::new(),
            plugin: package.name.clone(),
            connector: connector.into(),
            revision: 1,
            method,
            enabled: true,
            fields: BTreeMap::new(),
            secrets: BTreeMap::new(),
            grant: None,
            grant_revision: 1,
            identity: None,
            bound: None,
            state: State::NotConnected,
            checked_at_ms: None,
        }
    };
    input(&mut record, &declaration, values)?;
    record.method = method;
    record.bound = bound(db, profile, &info, &declaration, method)?;
    Ok((record, declaration))
}

fn next(record: &mut Record) -> Result<(), Fault> {
    record.revision = record
        .revision
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .ok_or_else(|| Fault::new(ErrorCode::Conflict, "connection revision exhausted"))?;
    Ok(())
}

pub(in crate::store) fn execute(
    db: &Connection,
    profile: Option<&std::path::Path>,
    request: &sailry_protocol::Request,
) -> Result<(Output, Option<Event>), Fault> {
    let command = &request.command;
    let owned = |name: &str| {
        request
            .plugin
            .as_ref()
            .is_none_or(|context| context.package.name == name)
    };
    match command {
        Command::ListConnectorTypes => {
            return Ok((
                Output::ConnectorTypes(
                    types(db)?
                        .into_iter()
                        .filter(|kind| owned(&kind.package.name))
                        .collect(),
                ),
                None,
            ));
        }
        Command::ListConnections => {
            return Ok((
                Output::Connections(
                    connections(db, profile)?
                        .into_iter()
                        .filter(|row| owned(&row.package.name))
                        .collect(),
                ),
                None,
            ));
        }
        _ => {}
    }
    let (id, expected) = match command {
        Command::UpdateConnection {
            connection,
            expected_revision,
            ..
        }
        | Command::SetConnectionEnabled {
            connection,
            expected_revision,
            ..
        }
        | Command::DisconnectConnection {
            connection,
            expected_revision,
        }
        | Command::RemoveConnection {
            connection,
            expected_revision,
        }
        | Command::ConfirmConnectorConsent {
            connection,
            expected_revision,
            ..
        } => (*connection, *expected_revision),
        _ => return Err(Fault::invalid("connector command expected")),
    };
    let mut record = read(db, id)?;
    super::super::commands::check_revision(record.revision, expected)?;
    let info = required(db, &record.plugin)?;
    let (_, declaration) = declaration(db, &info.summary.reference(), &record.connector)?;
    match command {
        Command::UpdateConnection { input: values, .. } => {
            input(&mut record, &declaration, values)?;
            record.grant = None;
            record.state = State::NotConnected;
        }
        Command::SetConnectionEnabled { enabled, .. } => record.enabled = *enabled,
        Command::DisconnectConnection { .. } => {
            record.secrets.clear();
            record.grant = None;
            record.state = State::NotConnected;
        }
        Command::RemoveConnection { .. } => {
            db.execute(
                "DELETE FROM plugin_connections WHERE id=?1",
                [id.to_string()],
            )
            .map_err(storage_error)?;
            return Ok((
                Output::Connection(project(db, profile, &record)?),
                Some(Event::PluginChanged(info.summary)),
            ));
        }
        Command::ConfirmConnectorConsent { accept, .. } => {
            if !accept {
                return Ok((Output::Connection(project(db, profile, &record)?), None));
            }
            let current = bound(db, profile, &info, &declaration, record.method)?;
            let reduced = record
                .bound
                .as_ref()
                .zip(current.as_ref())
                .is_some_and(|(old, new)| consent_reduced(old, new));
            if !reduced {
                record.grant = None;
                record.state = State::NotConnected;
            }
            record.bound = current;
        }
        _ => unreachable!(),
    }
    next(&mut record)?;
    save(db, &record)?;
    Ok((
        Output::Connection(project(db, profile, &record)?),
        Some(Event::PluginChanged(info.summary)),
    ))
}

/// Intrinsic access exposes only this package's public connection state and login.
pub(in crate::store) fn check(
    db: &Connection,
    request: &sailry_protocol::Request,
) -> Result<bool, Fault> {
    let Some(context) = &request.plugin else {
        return Ok(false);
    };
    let denied = || {
        Fault::new(
            ErrorCode::PermissionDenied,
            "connector belongs to another package or surface",
        )
    };
    match &request.command {
        Command::ListConnectorTypes | Command::ListConnections => {}
        Command::StartConnectorLogin {
            package,
            connector,
            method,
            input,
            connection,
            ..
        } => {
            if *package != context.package
                || context.turn.is_some()
                || context.invocation.is_some()
                || input.registration
                || !input.secrets.is_empty()
                || !matches!(method, Kind::Oauth2 | Kind::McpOauth)
            {
                return Err(denied());
            }
            declaration(db, package, connector)?;
            if let Some(id) = connection {
                let record = read(db, *id)?;
                if record.plugin != package.name || record.connector != *connector {
                    return Err(denied());
                }
            }
        }
        // The login owner binds input and cancellation to its original caller and scope.
        Command::SubmitConnectorInput { .. } | Command::CancelConnectorLogin { .. } => {}
        Command::InspectConnectorCli { .. }
        | Command::UpdateConnection { .. }
        | Command::SetConnectionEnabled { .. }
        | Command::DisconnectConnection { .. }
        | Command::RemoveConnection { .. }
        | Command::ConfirmConnectorConsent { .. }
        | Command::ProbeConnection { .. } => return Err(denied()),
        _ => return Ok(false),
    }
    Ok(true)
}

fn consent_reduced(old: &Bound, new: &Bound) -> bool {
    let mut reduced = old.clone();
    let scopes = match (&mut reduced.auth, &new.auth) {
        (Auth::Oauth2 { scopes: old, .. }, Auth::Oauth2 { scopes: new, .. }) => Some((old, new)),
        (
            Auth::Application {
                registration: Some(old),
                ..
            },
            Auth::Application {
                registration: Some(new),
                ..
            },
        ) => Some((&mut old.scopes, &new.scopes)),
        _ => None,
    };
    if let Some((old, new)) = scopes {
        if !new.iter().all(|scope| old.contains(scope)) {
            return false;
        }
        *old = new.clone();
    }
    if !new
        .origins
        .iter()
        .all(|origin| old.origins.contains(origin))
        || !new.inject.iter().all(|target| old.inject.contains(target))
        || !new
            .targets
            .iter()
            .all(|(name, target)| old.targets.get(name) == Some(target))
    {
        return false;
    }
    reduced.origins = new.origins.clone();
    reduced.inject = new.inject.clone();
    reduced.targets = new.targets.clone();
    reduced == *new
}

pub(in crate::store) fn capture(
    db: &Connection,
    turn: sailry_protocol::TurnId,
    packages: &[plugin::Reference],
) -> Result<(), Fault> {
    for record in records(db)? {
        if packages.iter().any(|package| package.name == record.plugin) {
            db.execute("INSERT INTO turn_connections(turn,plugin,connector,connection,revision) VALUES(?1,?2,?3,?4,?5)",params![turn.to_string(),record.plugin,record.connector,record.id.to_string(),record.revision as i64]).map_err(storage_error)?;
        }
    }
    Ok(())
}

pub(in crate::store) fn resolve(
    db: &Connection,
    profile: Option<&std::path::Path>,
    context: &plugin::Context,
    connector: &str,
    id: Option<Id>,
) -> Result<(Record, Declaration), Fault> {
    let record = records(db)?
        .into_iter()
        .find(|record| {
            record.plugin == context.package.name
                && record.connector == connector
                && id.is_none_or(|id| record.id == id)
        })
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "connector is not connected"))?;
    let info = if context.turn.is_some() || context.invocation.is_some() {
        crate::store::agent::plugins::packages(db, std::slice::from_ref(&context.package))?
            .pop()
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::NotConfigured,
                    "captured connector package is unavailable",
                )
            })?
    } else {
        settings::current(db, &context.package)?
    };
    let declaration = registry::declarations(&info)
        .into_iter()
        .find(|item| item.id == connector)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "connector is unavailable"))?;
    let live = project(db, profile, &record)?;
    let mut captured = bound(db, profile, &info, &declaration, record.method)?;
    if (context.turn.is_some() || context.invocation.is_some())
        && let Some(captured) = &mut captured
    {
        // Immutable package metadata omits installation provenance. The live
        // projection still verifies that provenance against the authorized record.
        captured.origin = record.bound.as_ref().and_then(|bound| bound.origin.clone());
    }
    if !record.enabled
        || !matches!(live.state, State::Connected | State::Failed)
        || record.bound != captured
    {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "connector is not connected",
        ));
    }
    if let Some(turn) = context.turn {
        let revision: Option<i64> = db
            .query_row(
                "SELECT revision FROM turn_connections WHERE turn=?1 AND connection=?2",
                params![turn.to_string(), record.id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if revision != Some(record.revision as i64) {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "connection changed",
            ));
        }
    }
    Ok((record, declaration))
}

pub(in crate::store) fn target(
    record: &Record,
    declaration: Declaration,
    package: plugin::Reference,
) -> crate::plugins::connectors::Target {
    crate::plugins::connectors::Target {
        id: record.id,
        revision: record.revision,
        package,
        declaration,
        method: record.method,
        fields: record.fields.clone(),
        secrets: record.secrets.clone(),
        identity: record.identity.clone(),
        grant: record
            .grant
            .as_ref()
            .map(|grant| crate::providers::login::plugin::Issued {
                account_label: None,
                access: grant.access.clone(),
                refresh: grant.refresh.clone(),
                expires_at_ms: grant.expires_at_ms,
                extra: grant.extra.clone(),
            }),
        mcp_endpoint: None,
        manual: false,
        checking: false,
    }
}

pub(in crate::store) fn current_target(
    db: &Connection,
    profile: Option<&std::path::Path>,
    id: Id,
    revision: u64,
    checking: bool,
) -> Result<crate::plugins::connectors::Target, Fault> {
    let record = read(db, id)?;
    super::super::commands::check_revision(record.revision, revision)?;
    let info = required(db, &record.plugin)?;
    let (_, declaration) = declaration(db, &info.summary.reference(), &record.connector)?;
    let state = project(db, profile, &record)?.state;
    if !record.enabled
        || !(matches!(state, State::Connected | State::Failed)
            || checking && state == State::Expired)
    {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "connector is not connected",
        ));
    }
    let mut target = target(&record, declaration, info.summary.reference());
    target.checking = checking;
    target.manual = info.mcp_source.is_some();
    target.mcp_endpoint = record.bound.as_ref().and_then(|bound| {
        let server = match &bound.auth {
            Auth::McpOauth { server, .. } => Some(server),
            _ => None,
        }?;
        match bound.targets.get(server)? {
            plugin::mcp::Definition::StreamableHttp { url, .. }
            | plugin::mcp::Definition::Sse { url, .. } => Some(url.clone()),
            _ => None,
        }
    });
    Ok(target)
}

pub(in crate::store) fn renew(
    database: &mut crate::store::database::Database,
    id: Id,
    revision: u64,
    previous: Option<&Secret>,
    issued: Option<crate::providers::login::plugin::Issued>,
    events: &tokio::sync::broadcast::Sender<sailry_protocol::EventEnvelope>,
) -> Result<(), Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let db = &transaction;
    let mut record = read(db, id)?;
    super::super::commands::check_revision(record.revision, revision)?;
    if record.grant.as_ref().map(|grant| &grant.access) != previous {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "connector grant changed",
        ));
    }
    record.grant_revision = record
        .grant_revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| Fault::new(ErrorCode::Conflict, "connector grant revision exhausted"))?;
    record.grant = issued.map(|issued| Grant {
        access: issued.access,
        refresh: issued.refresh,
        expires_at_ms: issued.expires_at_ms,
        extra: issued.extra,
    });
    if record.grant.is_none() {
        record.state = State::Expired;
    }
    save(db, &record)?;
    let envelopes = crate::store::database::record(
        db,
        database.node,
        vec![Event::PluginChanged(required(db, &record.plugin)?.summary)],
    )?;
    transaction.commit().map_err(storage_error)?;
    for envelope in envelopes {
        let _ = events.send(envelope);
    }
    Ok(())
}

/// Only the current settings revision is moved; historical captures remain untouched.
pub(in crate::store) fn needs_migration(db: &Connection, candidate: &Info) -> Result<bool, Fault> {
    let Some(info) = get(db, &candidate.summary.name)? else {
        return Ok(false);
    };
    let saved = settings::stored(db, &info)?;
    let records = records(db)?;
    Ok(registry::declarations(candidate).iter().any(|declaration| {
        !records
            .iter()
            .any(|record| record.plugin == info.summary.name && record.connector == declaration.id)
            && (declaration.fields.iter().any(|field| {
                saved.values.contains_key(&field.name)
                    || saved.slots.get(&field.name).is_some_and(Option::is_some)
            }) || declaration.auth.iter().any(|auth| match auth {
                Auth::Oauth2 { .. } => saved
                    .oauth
                    .as_ref()
                    .and_then(|saved| saved.grant.as_ref())
                    .is_some(),
                Auth::McpOauth { server, .. } => saved
                    .authorizations
                    .get(server)
                    .is_some_and(|grant| grant.credentials.is_some()),
                Auth::LocalCli { .. } => saved.github.is_some(),
                _ => false,
            }))
    }))
}

pub(super) fn migrate(
    db: &Connection,
    profile: Option<&std::path::Path>,
    info: &mut Info,
) -> Result<bool, Fault> {
    let declarations = registry::declarations(info);
    if declarations.is_empty() {
        return Ok(false);
    }
    let mut stored = settings::stored(db, info)?;
    let mut changed = false;
    for declaration in declarations {
        if records(db)?
            .iter()
            .any(|record| record.plugin == info.summary.name && record.connector == declaration.id)
        {
            continue;
        }
        let method=if stored.github.is_some() && !declaration.fields.iter().any(|field| field.secret && stored.slots.get(&field.name).is_some_and(Option::is_some)) && declaration.auth.iter().any(|auth|auth.kind()==Kind::LocalCli){Kind::LocalCli}
            else if stored.oauth.as_ref().and_then(|authorization|authorization.grant.as_ref()).is_some() && declaration.auth.iter().any(|auth|auth.kind()==Kind::Oauth2){Kind::Oauth2}
            else if declaration.auth.iter().any(|auth|matches!(auth,Auth::McpOauth{server,..} if stored.authorizations.get(server).is_some_and(|grant|grant.credentials.is_some()))){Kind::McpOauth}
            else if declaration.auth.iter().any(|auth|auth.kind()==Kind::Application){Kind::Application}
            else {declaration.auth.iter().find(|auth| auth.kind() == Kind::Secret).or_else(|| declaration.auth.first()).map(Auth::kind).ok_or_else(|| Fault::invalid("connector authentication method is unavailable"))?};
        let mut record = Record {
            id: RequestId::new(),
            plugin: info.summary.name.clone(),
            connector: declaration.id.clone(),
            revision: 1,
            method,
            enabled: true,
            fields: BTreeMap::new(),
            secrets: BTreeMap::new(),
            grant: None,
            grant_revision: 1,
            identity: None,
            bound: bound(db, profile, info, &declaration, method)?,
            state: State::NotConnected,
            checked_at_ms: None,
        };
        for field in &declaration.fields {
            if field.secret {
                if let Some(secret) = stored.slots.get(&field.name).cloned().flatten() {
                    record.secrets.insert(field.name.clone(), secret);
                }
            } else if let Some(value) = stored
                .values
                .get(&field.name)
                .and_then(serde_json::Value::as_str)
            {
                record.fields.insert(field.name.clone(), value.into());
            }
        }
        input(&mut record, &declaration, &Input::default())?;
        match method {
            Kind::LocalCli => {
                if let Some(identity) = &stored.github {
                    record.identity = Some(Identity {
                        name: identity.login.clone(),
                        id: identity.login.clone(),
                    });
                    record.secrets.clear();
                    record.state = State::Connected;
                }
            }
            Kind::Oauth2 => {
                if let Some(grant) = stored
                    .oauth
                    .as_ref()
                    .and_then(|authorization| authorization.grant.as_ref())
                {
                    record
                        .fields
                        .insert("client_id".into(), grant.configuration.client_id.clone());
                    if let Some(secret) = &grant.configuration.client_secret {
                        record
                            .secrets
                            .insert("client_secret".into(), secret.clone());
                    }
                    if let Some(tenant) = &grant.configuration.tenant {
                        record.fields.insert("tenant".into(), tenant.clone());
                    }
                    record.grant = Some(Grant {
                        access: grant.access_token.clone(),
                        refresh: grant.refresh_token.clone(),
                        expires_at_ms: Some(grant.expires_at_ms),
                        extra: BTreeMap::new(),
                    });
                    record.state = State::Connected;
                }
            }
            Kind::McpOauth => {
                if let Some(Auth::McpOauth { server, .. }) =
                    declaration.auth.iter().find(|auth| auth.kind() == method)
                    && let Some(credentials) = stored
                        .authorizations
                        .get(server)
                        .and_then(|grant| grant.credentials.clone())
                {
                    record.grant = Some(Grant {
                        access: credentials,
                        refresh: None,
                        expires_at_ms: None,
                        extra: BTreeMap::new(),
                    });
                    record.state = State::Connected;
                }
            }
            Kind::Secret => {
                if let Some(Auth::Secret { fields }) =
                    declaration.auth.iter().find(|auth| auth.kind() == method)
                    && fields
                        .iter()
                        .all(|field| record.secrets.contains_key(field))
                {
                    record.state = State::Connected;
                }
            }
            Kind::Application => {
                if declaration
                    .fields
                    .iter()
                    .filter(|field| !field.optional)
                    .all(|field| {
                        if field.secret {
                            record.secrets.contains_key(&field.name)
                        } else {
                            record.fields.contains_key(&field.name)
                        }
                    })
                {
                    record.state = State::Connected;
                }
            }
        }
        if record.fields.is_empty()
            && record.secrets.is_empty()
            && record.grant.is_none()
            && record.identity.is_none()
        {
            continue;
        }
        save(db, &record)?;
        for field in &declaration.fields {
            stored.values.remove(&field.name);
            stored.slots.remove(&field.name);
        }
        if method == Kind::Oauth2 {
            if let Some(grant) = stored.oauth.as_ref().and_then(|saved| saved.grant.as_ref()) {
                for name in &grant.declaration.credentials {
                    stored.slots.remove(name);
                }
            }
            stored.oauth = None;
        }
        if method == Kind::LocalCli {
            stored.github = None;
        }
        if let Some(Auth::McpOauth { server, .. }) =
            declaration.auth.iter().find(|auth| auth.kind() == method)
        {
            stored.authorizations.remove(server);
        }
        changed = true;
    }
    if changed {
        stored.schema = info.settings.clone();
        settings::persist(db, info, &stored)?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests;
