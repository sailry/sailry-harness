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
pub(in crate::store) mod consent;
mod editing;
pub(in crate::store) mod login;
mod migration;
pub(super) use migration::migrate;
pub(in crate::store) use migration::needed as needs_migration;
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
    pub auth_revision: u64,
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
    #[serde(default)]
    pub login: Option<login::Pending>,
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

pub(in crate::store) async fn types(db: &Connection) -> Result<Vec<Type>, Fault> {
    let mut result = Vec::new();
    for summary in list(db).await? {
        let info = required(db, &summary.name).await?;
        for declaration in registry::declarations(&info) {
            result.push(Type {
                package: summary.reference(),
                declaration,
                enabled: summary.enabled,
            });
        }
    }
    Ok(result)
}

pub(in crate::store) async fn info(db: &Connection, name: &str) -> Result<Info, Fault> {
    required(db, name).await
}

pub(in crate::store) async fn declaration(
    db: &Connection,
    package: &plugin::Reference,
    connector: &str,
) -> Result<(Info, Declaration), Fault> {
    let info = settings::current(db, package).await?;
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

pub(in crate::store) async fn bound(
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
        targets: targets(db, profile, info, declaration).await?,
    }))
}

async fn targets(
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
    let configuration = match settings::stored(db, info).await?.mcp {
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
                plugin::mcp::Server::Http { url, headers, .. }
                | plugin::mcp::Server::Sse { url, headers, .. } => {
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

pub(in crate::store) async fn records(db: &Connection) -> Result<Vec<Record>, Fault> {
    let mut query = db
        .prepare("SELECT body FROM plugin_connections ORDER BY plugin,connector,id")
        .await
        .map_err(storage_error)?;
    (async {
        let mut rows = query.query(()).await?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await? {
            let mapped: turso::Result<_> = row.get::<Vec<u8>>(0);
            values.push(mapped);
        }
        Ok::<_, turso::Error>(values.into_iter())
    })
    .await
    .map_err(storage_error)?
    .map(|bytes| serde_json::from_slice(&bytes.map_err(storage_error)?).map_err(storage_error))
    .collect()
}

pub(in crate::store) async fn read(db: &Connection, id: Id) -> Result<Record, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM plugin_connections WHERE id=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([id.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    serde_json::from_slice(
        &body.ok_or_else(|| Fault::new(ErrorCode::NotFound, "connection is unavailable"))?,
    )
    .map_err(storage_error)
}

pub(in crate::store) async fn save(db: &Connection, record: &Record) -> Result<(), Fault> {
    validate(record)?;
    db.execute("INSERT INTO plugin_connections(id,plugin,connector,revision,body) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,body=excluded.body WHERE plugin=excluded.plugin AND connector=excluded.connector", params![record.id.to_string(),record.plugin.as_str(),record.connector.as_str(),record.revision as i64,encode(record)?]).await.map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) fn validate(record: &Record) -> Result<(), Fault> {
    if record.revision == 0
        || record.revision > i64::MAX as u64
        || record.auth_revision == 0
        || record.auth_revision > record.revision
        || !identifier(&record.connector)
        || crate::plugins::validate_name(&record.plugin).is_err()
        || record.grant_revision == 0
        || record.grant_revision > i64::MAX as u64
        || record.fields.len() > 8
        || record.secrets.len() > 8
        || !login::valid(record)
        || (record.method == Kind::LocalCli
            && (record.grant.is_some() || !record.secrets.is_empty()))
    {
        return Err(Fault::invalid("invalid stored connection"));
    }
    Ok(())
}

pub(in crate::store) async fn project(
    db: &Connection,
    profile: Option<&std::path::Path>,
    record: &Record,
) -> Result<Projection, Fault> {
    let info = get(db, &record.plugin)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "connector package is unavailable"))?;
    let declaration = registry::declarations(&info)
        .into_iter()
        .find(|item| item.id == record.connector);
    let current = match declaration.as_ref() {
        Some(declaration) => bound(db, profile, &info, declaration, record.method).await?,
        None => None,
    };
    let consent = consent::changes(&record.plugin, record.bound.as_ref(), current.as_ref());
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
        _ => authorization(record),
    };
    Ok(Projection {
        id: record.id,
        package: info.summary.reference(),
        connector: record.connector.clone(),
        method: record.method,
        revision: record.revision,
        enabled: record.enabled,
        state,
        can_disconnect: record.state != State::Connecting
            && record.login.is_none()
            && (record.grant.is_some()
                || record
                    .secrets
                    .values()
                    .any(|secret| !secret.expose().is_empty())
                || record.method == Kind::LocalCli
                    && record.state != State::NotConnected
                    && record.identity.is_some()),
        identity: record.identity.clone(),
        expires_at_ms: record.grant.as_ref().and_then(|grant| grant.expires_at_ms),
        checked_at_ms: record.checked_at_ms,
        fields: record.fields.clone(),
        configured: record.secrets.keys().cloned().collect(),
        consent,
    })
}

/// MCP grants have an opaque SDK-owned payload. Only the connection owner's
/// explicit state and grant metadata determine whether authorization is usable.
fn authorization(record: &Record) -> State {
    if record.method != Kind::McpOauth {
        return record.state;
    }
    if record.login.is_some() {
        return State::Connecting;
    }
    if record.state != State::Connected {
        return record.state;
    }
    match &record.grant {
        None => State::NotConnected,
        Some(grant) if grant.access.expose().is_empty() => State::NotConnected,
        Some(grant)
            if grant
                .expires_at_ms
                .is_some_and(|expires| expires <= crate::plugins::connectors::now()) =>
        {
            State::Expired
        }
        Some(_) => State::Connected,
    }
}

pub(in crate::store) async fn connections(
    db: &Connection,
    profile: Option<&std::path::Path>,
) -> Result<Vec<Projection>, Fault> {
    let types = types(db).await?;
    let mut output = Vec::new();
    for record in records(db).await? {
        if types.iter().any(|kind| {
            kind.package.name == record.plugin && kind.declaration.id == record.connector
        }) {
            output.push(project(db, profile, &record).await?);
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

pub(in crate::store) async fn create(
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
    let (info, declaration) = declaration(db, package, connector).await?;
    if !declaration.auth.iter().any(|auth| auth.kind() == method) {
        return Err(Fault::invalid(
            "connector authentication method is unavailable",
        ));
    }
    let mut record = if let Some(id) = id {
        let record = read(db, id).await?;
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
        if records(db)
            .await?
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
            auth_revision: 1,
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
            login: None,
        }
    };
    let previous = record.clone();
    input(&mut record, &declaration, values)?;
    record.method = method;
    record.bound = bound(db, profile, &info, &declaration, method).await?;
    if id.is_some()
        && (editing::changed(&previous, &record, &declaration)
            || !consent::same(
                &record.plugin,
                previous.bound.as_ref(),
                record.bound.as_ref(),
            ))
    {
        revoke(&mut record)?;
        record.grant = None;
    }
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

fn revoke(record: &mut Record) -> Result<(), Fault> {
    record.grant_revision = record
        .grant_revision
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .ok_or_else(|| Fault::new(ErrorCode::Conflict, "connector grant revision exhausted"))?;
    record.auth_revision = record
        .revision
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::Conflict,
                "connection authorization revision exhausted",
            )
        })?;
    record.login = None;
    if record.state == State::Connecting {
        record.state = State::NotConnected;
    }
    Ok(())
}

fn enabled(record: &mut Record, value: bool) -> Result<(), Fault> {
    if record.enabled != value {
        revoke(record)?;
        record.enabled = value;
    }
    Ok(())
}

pub(in crate::store) async fn execute(
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
                    types(db)
                        .await?
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
                    connections(db, profile)
                        .await?
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
    let mut record = read(db, id).await?;
    super::super::commands::check_revision(record.revision, expected)?;
    let info = required(db, &record.plugin).await?;
    // Public account editing remains available while its package is disabled.
    let declaration = registry::declarations(&info)
        .into_iter()
        .find(|declaration| declaration.id == record.connector)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "connector is unavailable"))?;
    match command {
        Command::UpdateConnection { input: values, .. } => {
            let previous = record.clone();
            input(&mut record, &declaration, values)?;
            if editing::changed(&previous, &record, &declaration) {
                revoke(&mut record)?;
                record.grant = None;
                record.state = State::NotConnected;
            }
        }
        Command::SetConnectionEnabled { enabled, .. } => {
            self::enabled(&mut record, *enabled)?;
        }
        Command::DisconnectConnection { .. } => {
            revoke(&mut record)?;
            record.secrets.clear();
            record.grant = None;
            record.state = State::NotConnected;
        }
        Command::RemoveConnection { .. } => {
            db.execute(
                "DELETE FROM plugin_connections WHERE id=?1",
                [id.to_string()],
            )
            .await
            .map_err(storage_error)?;
            return Ok((
                Output::Connection(project(db, profile, &record).await?),
                Some(Event::PluginChanged(info.summary)),
            ));
        }
        Command::ConfirmConnectorConsent { accept, .. } => {
            if !accept {
                return Ok((
                    Output::Connection(project(db, profile, &record).await?),
                    None,
                ));
            }
            let current = bound(db, profile, &info, &declaration, record.method).await?;
            let reduced = record
                .bound
                .as_ref()
                .zip(current.as_ref())
                .is_some_and(|(old, new)| consent_reduced(&record.plugin, old, new));
            if !consent::same(&record.plugin, record.bound.as_ref(), current.as_ref()) {
                revoke(&mut record)?;
            }
            if !reduced {
                record.grant = None;
                record.state = State::NotConnected;
            }
            record.bound = current;
        }
        _ => unreachable!(),
    }
    next(&mut record)?;
    save(db, &record).await?;
    Ok((
        Output::Connection(project(db, profile, &record).await?),
        Some(Event::PluginChanged(info.summary)),
    ))
}

/// Intrinsic access exposes only this package's public connection state and login.
pub(in crate::store) async fn check(
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
            declaration(db, package, connector).await?;
            if let Some(id) = connection {
                let record = read(db, *id).await?;
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

fn consent_reduced(package: &str, old: &Bound, new: &Bound) -> bool {
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
    consent::same(package, Some(&reduced), Some(new))
}

pub(in crate::store) async fn capture(
    db: &Connection,
    turn: sailry_protocol::TurnId,
    packages: &[plugin::Reference],
) -> Result<(), Fault> {
    for record in records(db).await? {
        if packages.iter().any(|package| package.name == record.plugin) {
            db.execute("INSERT INTO turn_connections(turn,plugin,connector,connection,revision) VALUES(?1,?2,?3,?4,?5)",params![turn.to_string(),record.plugin,record.connector,record.id.to_string(),record.auth_revision as i64]).await.map_err(storage_error)?;
        }
    }
    Ok(())
}

pub(in crate::store) async fn resolve(
    db: &Connection,
    profile: Option<&std::path::Path>,
    context: &plugin::Context,
    connector: &str,
    id: Option<Id>,
) -> Result<(Record, Declaration), Fault> {
    let record = records(db)
        .await?
        .into_iter()
        .find(|record| {
            record.plugin == context.package.name
                && record.connector == connector
                && id.is_none_or(|id| record.id == id)
        })
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "connector is not connected"))?;
    let info = if context.turn.is_some() || context.invocation.is_some() {
        crate::store::agent::plugins::packages(db, std::slice::from_ref(&context.package))
            .await?
            .pop()
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::NotConfigured,
                    "captured connector package is unavailable",
                )
            })?
    } else {
        settings::current(db, &context.package).await?
    };
    let declaration = registry::declarations(&info)
        .into_iter()
        .find(|item| item.id == connector)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "connector is unavailable"))?;
    let live = project(db, profile, &record).await?;
    let mut captured = bound(db, profile, &info, &declaration, record.method).await?;
    if (context.turn.is_some() || context.invocation.is_some())
        && let Some(captured) = &mut captured
    {
        // Immutable package metadata omits installation provenance. The live
        // projection still verifies that provenance against the authorized record.
        captured.origin = record.bound.as_ref().and_then(|bound| bound.origin.clone());
    }
    if !record.enabled
        || !(matches!(live.state, State::Connected | State::Failed)
            || live.state == State::Connecting && login::usable(&record))
        || !consent::same(&record.plugin, record.bound.as_ref(), captured.as_ref())
    {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "connector is not connected",
        ));
    }
    if let Some(turn) = context.turn {
        let revision: Option<i64> = {
            let mut statement = db
                .prepare("SELECT revision FROM turn_connections WHERE turn=?1 AND connection=?2")
                .await
                .map_err(storage_error)?;
            match statement
                .query_row(params![turn.to_string(), record.id.to_string()])
                .await
            {
                Ok(row) => {
                    let mapped: turso::Result<_> = row.get(0);
                    Some(mapped.map_err(storage_error)?)
                }
                Err(turso::Error::QueryReturnedNoRows) => None,
                Err(error) => return Err(storage_error(error)),
            }
        };
        if revision != Some(record.auth_revision as i64) {
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
        revision: record.auth_revision,
        grant_revision: record.grant_revision,
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

pub(in crate::store) async fn current_target(
    db: &Connection,
    profile: Option<&std::path::Path>,
    id: Id,
    revision: u64,
    checking: bool,
) -> Result<crate::plugins::connectors::Target, Fault> {
    let record = read(db, id).await?;
    super::super::commands::check_revision(record.auth_revision, revision)?;
    let info = required(db, &record.plugin).await?;
    let (_, declaration) = declaration(db, &info.summary.reference(), &record.connector).await?;
    let state = project(db, profile, &record).await?.state;
    if !record.enabled
        || !(matches!(state, State::Connected | State::Failed)
            || state == State::Connecting && login::usable(&record)
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

pub(in crate::store) async fn renew(
    db: &Connection,
    id: Id,
    revision: u64,
    grant_revision: u64,
    issued: Option<crate::providers::login::plugin::Issued>,
) -> Result<Record, Fault> {
    let mut record = read(db, id).await?;
    super::super::commands::check_revision(record.auth_revision, revision)?;
    super::super::commands::check_revision(record.grant_revision, grant_revision)?;
    record.grant_revision = record
        .grant_revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| Fault::new(ErrorCode::Conflict, "connector grant revision exhausted"))?;
    let expired = issued.is_none();
    record.grant = issued
        .filter(|_| record.method != Kind::LocalCli)
        .map(|issued| Grant {
            access: issued.access,
            refresh: issued.refresh,
            expires_at_ms: issued.expires_at_ms,
            extra: issued.extra,
        });
    if expired {
        record.state = State::Expired;
    }
    save(db, &record).await?;
    Ok(record)
}

pub(in crate::store) async fn save_grant(
    database: &mut crate::store::database::Database,
    id: Id,
    revision: u64,
    grant_revision: u64,
    issued: Option<crate::providers::login::plugin::Issued>,
    events: &tokio::sync::broadcast::Sender<sailry_protocol::EventEnvelope>,
) -> Result<u64, Fault> {
    let publication = database.publication.clone();
    let node = database.node;
    let transaction = database
        .connection
        .transaction()
        .await
        .map_err(storage_error)?;
    let db = &transaction;
    let record = renew(db, id, revision, grant_revision, issued).await?;
    let envelopes = crate::store::database::record(
        db,
        node,
        vec![Event::PluginChanged(
            required(db, &record.plugin).await?.summary,
        )],
    )
    .await?;
    let publication = publication.lock().await;
    crate::store::database::conflict::commit(transaction).await?;
    for envelope in envelopes {
        publication.send(events, envelope);
    }
    Ok(record.grant_revision)
}

/// A standalone MCP package owns only its own active connection records.
pub(super) async fn remove_mcp(db: &Connection, info: &Info) -> Result<(), Fault> {
    if info.mcp_source.is_some() {
        db.execute(
            "DELETE FROM plugin_connections WHERE plugin=?1",
            [info.summary.name.as_str()],
        )
        .await
        .map_err(storage_error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
