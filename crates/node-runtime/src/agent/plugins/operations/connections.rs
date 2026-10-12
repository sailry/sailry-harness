//! Connection operations retain the admitted choices and existing Node commands.
use super::*;
use crate::store::agent::connections::Action;
use sailry_protocol::connection::Resource;
use serde::Deserialize;

pub(super) fn handles(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::ListDatabaseConnections
            | Operation::SaveDatabaseConnection
            | Operation::ListSshConnections
            | Operation::SaveSshConnection
            | Operation::BrowseDatabase
            | Operation::QueryDatabase
            | Operation::ExecuteDatabase
            | Operation::RunSsh
            | Operation::TransferSsh
    )
}

pub(super) fn description(operation: Operation) -> &'static str {
    match operation {
        Operation::ListDatabaseConnections => "List public database connection configuration",
        Operation::SaveDatabaseConnection => {
            "Save database connection configuration without a password"
        }
        Operation::ListSshConnections => "List public SSH connection configuration",
        Operation::SaveSshConnection => {
            "Save SSH connection configuration and explicitly supplied credentials"
        }
        Operation::BrowseDatabase => "Read a catalog from an admitted database connection",
        Operation::QueryDatabase => "Query an admitted database through a read-only connection",
        Operation::ExecuteDatabase => "Execute SQL on an admitted database connection",
        Operation::RunSsh => "Run a command through an admitted SSH connection",
        Operation::TransferSsh => "Transfer a file through an admitted SSH connection",
        _ => unreachable!(),
    }
}

pub(super) fn parameters(operation: Operation) -> Value {
    if matches!(
        operation,
        Operation::ListDatabaseConnections | Operation::ListSshConnections
    ) {
        return json!({"type":"object","properties":{},"additionalProperties":false});
    }
    if matches!(
        operation,
        Operation::SaveDatabaseConnection | Operation::SaveSshConnection
    ) {
        let mut properties = json!({"connection":{"type":["string","null"]},"expected_revision":{"type":["integer","null"],"minimum":1},"name":{"type":"string"},"host":{"type":"string"},"port":{"type":"integer","minimum":1,"maximum":65535},"username":{"type":"string"}});
        if operation == Operation::SaveDatabaseConnection {
            properties["engine"] = json!({"enum":["sqlite","mysql","postgres"]});
            properties["path"] = json!({"type":"string","description":"SQLite file path on the execution Node; relative paths use the captured worktree. Saving does not create the file"});
            properties["database"] = json!({"type":"string"});
            properties["read_only"] = json!({"type":"boolean"});
        } else {
            properties["authentication"] =
                json!({"enum":["password","private_key","key_path","agent"]});
            properties["key_path"] = json!({"type":"string"});
            properties["credential"] = json!({"oneOf":[
                {"type":"object","properties":{"kind":{"const":"password"},"password":{"type":"string"}},"required":["kind","password"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"private_key"},"key":{"type":"string"},"passphrase":{"type":["string","null"]}},"required":["kind","key"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"key_path"},"path":{"type":"string"},"passphrase":{"type":["string","null"]}},"required":["kind","path"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"agent"}},"required":["kind"],"additionalProperties":false}
            ]});
        }
        return json!({"type":"object","properties":properties,"additionalProperties":false});
    }
    let mut properties = json!({"connection":{"type":"string"}});
    let mut required = vec!["connection"];
    match operation {
        Operation::BrowseDatabase | Operation::QueryDatabase | Operation::ExecuteDatabase => {
            properties["database"] = json!({"type":["string","null"]});
            if operation != Operation::BrowseDatabase {
                properties["sql"] = json!({"type":"string"});
                properties["row_limit"] = json!({"type":"integer","minimum":1,"maximum":10000});
                properties["timeout_ms"] = json!({"type":"integer","minimum":1,"maximum":3600000});
                required.extend(["sql", "row_limit", "timeout_ms"]);
            }
        }
        Operation::RunSsh => {
            properties["command"] = json!({"type":"string"});
            properties["timeout_ms"] = json!({"type":"integer","minimum":1,"maximum":900000});
            required.extend(["command", "timeout_ms"]);
        }
        Operation::TransferSsh => {
            properties["path"] = json!({"type":"string"});
            properties["remote_path"] = json!({"type":"string"});
            properties["direction"] = json!({"enum":["upload","download"]});
            properties["timeout_ms"] = json!({"type":"integer","minimum":1,"maximum":900000});
            required.extend(["path", "remote_path", "direction", "timeout_ms"]);
        }
        _ => unreachable!(),
    }
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub(super) fn action(
    operation: Operation,
    arguments: Value,
) -> Result<(Option<Resource>, Action), Fault> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Catalog {
        connection: sailry_protocol::DatabaseId,
        database: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Query {
        connection: sailry_protocol::DatabaseId,
        database: Option<String>,
        sql: String,
        row_limit: u32,
        timeout_ms: u64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Run {
        connection: sailry_protocol::SshId,
        command: String,
        timeout_ms: u64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Transfer {
        connection: sailry_protocol::SshId,
        path: String,
        remote_path: String,
        direction: sailry_protocol::ssh::Direction,
        timeout_ms: u64,
    }
    fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, Fault> {
        serde_json::from_value(value)
            .map_err(|error| Fault::invalid(format!("invalid connection operation: {error}")))
    }
    if management(operation)
        && arguments.as_object().is_some_and(|fields| {
            fields.iter().any(|(name, value)| {
                value.is_null() && !matches!(name.as_str(), "connection" | "expected_revision")
            })
        })
    {
        return Err(Fault::invalid("connection edit fields cannot be null"));
    }
    Ok(match operation {
        Operation::ListDatabaseConnections | Operation::ListSshConnections => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Empty {}
            let _: Empty = parse(arguments)?;
            (
                None,
                if operation == Operation::ListDatabaseConnections {
                    Action::ListDatabases
                } else {
                    Action::ListSsh
                },
            )
        }
        Operation::SaveDatabaseConnection => (None, Action::SaveDatabase(parse(arguments)?)),
        Operation::SaveSshConnection => {
            if let Some(credential) = arguments.get("credential") {
                let fields = credential
                    .as_object()
                    .ok_or_else(|| Fault::invalid("invalid SSH credential"))?;
                let allowed: &[&str] = match fields.get("kind").and_then(Value::as_str) {
                    Some("password") => &["kind", "password"],
                    Some("private_key") => &["kind", "key", "passphrase"],
                    Some("key_path") => &["kind", "path", "passphrase"],
                    Some("agent") => &["kind"],
                    _ => return Err(Fault::invalid("invalid SSH credential")),
                };
                if fields.keys().any(|name| !allowed.contains(&name.as_str())) {
                    return Err(Fault::invalid("invalid SSH credential"));
                }
            }
            let input = serde_json::from_value(arguments)
                .map_err(|_| Fault::invalid("invalid SSH connection operation"))?;
            (None, Action::SaveSsh(input))
        }
        Operation::BrowseDatabase => {
            let input: Catalog = parse(arguments)?;
            (
                Some(Resource::Database(input.connection)),
                Action::Catalog {
                    database: input.database,
                },
            )
        }
        Operation::QueryDatabase | Operation::ExecuteDatabase => {
            let input: Query = parse(arguments)?;
            (
                Some(Resource::Database(input.connection)),
                Action::Query {
                    read_only: operation == Operation::QueryDatabase,
                    database: input.database,
                    sql: input.sql,
                    row_limit: input.row_limit,
                    timeout_ms: input.timeout_ms,
                },
            )
        }
        Operation::RunSsh => {
            let input: Run = parse(arguments)?;
            (
                Some(Resource::Ssh(input.connection)),
                Action::Run {
                    command: input.command,
                    timeout_ms: Some(input.timeout_ms),
                },
            )
        }
        Operation::TransferSsh => {
            let input: Transfer = parse(arguments)?;
            (
                Some(Resource::Ssh(input.connection)),
                Action::Transfer {
                    path: input.path,
                    remote_path: input.remote_path,
                    direction: input.direction,
                    timeout_ms: input.timeout_ms,
                },
            )
        }
        _ => unreachable!(),
    })
}

pub(super) fn management(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::SaveDatabaseConnection | Operation::SaveSshConnection
    )
}
pub(super) fn ssh(operation: Operation) -> bool {
    matches!(operation, Operation::RunSsh | Operation::TransferSsh)
}

pub(super) async fn preflight(
    owner: &HostOperation,
    request: &Request,
) -> Result<Option<sailry_protocol::ssh::Trust>, Fault> {
    let (profile, expected_revision) = match &request.command {
        Command::RunSsh {
            profile,
            expected_revision,
            ..
        }
        | Command::TransferSsh {
            profile,
            expected_revision,
            ..
        } => (*profile, *expected_revision),
        _ => return Ok(None),
    };
    let check = Request::new(
        owner.transport.target(),
        Command::CheckSsh {
            profile,
            expected_revision,
        },
    )
    .with_plugin(owner.context.clone());
    owner
        .ingress
        .check_plugin(owner.caller, check.clone())
        .await?;
    let id = check.id;
    let admission = owner.transport.dispatch(check).await?;
    let output = tokio::select! {
        biased;
        _ = owner.stop.cancelled() => {
            let _ = owner.transport.dispatch(Request::new(owner.transport.target(), Command::CancelSsh { request: id }).with_plugin(owner.context.clone())).await;
            return Err(Fault::new(ErrorCode::Cancelled,"SSH host key inspection was cancelled"));
        }
        result = admission.completion => result.map_err(|_| Fault::new(ErrorCode::OutcomeUnknown,"SSH host key inspection outcome is unavailable"))??,
    };
    match output {
        sailry_protocol::Output::SshOutcome(sailry_protocol::ssh::Outcome::HostKeyRequired {
            key,
            changed,
        }) => owner
            .ingress
            .connection_trust(
                owner.turn,
                owner.context.package.clone(),
                profile,
                expected_revision,
                key,
                changed,
                owner.connections.clone(),
            )
            .await
            .map(Some),
        sailry_protocol::Output::SshOutcome(sailry_protocol::ssh::Outcome::Connected) => Ok(None),
        _ => Err(Fault::new(
            ErrorCode::Internal,
            "SSH host key inspection returned an unexpected result",
        )),
    }
}

pub(super) async fn trust(
    owner: &HostOperation,
    trust: &sailry_protocol::ssh::Trust,
) -> Result<(), Fault> {
    let request = Request::new(
        owner.transport.target(),
        Command::TrustSsh {
            profile: trust.profile,
            expected_revision: trust.expected_revision,
            key: trust.key.clone(),
        },
    )
    .with_plugin(owner.context.clone());
    owner
        .ingress
        .check_plugin(owner.caller, request.clone())
        .await?;
    let admission = owner.transport.dispatch(request).await?;
    admission.completion.await.map_err(|_| {
        Fault::new(
            ErrorCode::OutcomeUnknown,
            "SSH host key save outcome is unavailable",
        )
    })??;
    Ok(())
}

pub(super) async fn annotate(
    owner: &HostOperation,
    output: &sailry_protocol::Output,
    value: &mut Value,
) -> Result<(), Fault> {
    let (targets, databases): (Vec<_>, bool) = match output {
        sailry_protocol::Output::DatabaseProfile(profile) => {
            (vec![Resource::Database(profile.id)], true)
        }
        sailry_protocol::Output::DatabaseProfiles(profiles) => (
            profiles.iter().map(|p| Resource::Database(p.id)).collect(),
            true,
        ),
        sailry_protocol::Output::SshProfile(profile) => (vec![Resource::Ssh(profile.id)], false),
        sailry_protocol::Output::SshProfiles(profiles) => (
            profiles.iter().map(|p| Resource::Ssh(p.id)).collect(),
            false,
        ),
        _ => return Ok(()),
    };
    let status = owner
        .ingress
        .connection_status(
            owner.turn,
            owner.context.package.clone(),
            targets,
            owner.connections.clone(),
        )
        .await?;
    let resource = |profile: &Value| -> Option<Resource> {
        let id = profile["id"].as_str()?;
        if databases {
            id.parse().ok().map(Resource::Database)
        } else {
            id.parse().ok().map(Resource::Ssh)
        }
    };
    if let Some(profiles) = value["data"].as_array_mut() {
        profiles.retain(|profile| resource(profile).is_some_and(|id| status.contains_key(&id)));
        if databases {
            for profile in profiles {
                let id = resource(profile).expect("filtered profile");
                profile["credentials_required"] = json!(status[&id]);
            }
        }
    } else if databases {
        let id = resource(&value["data"])
            .ok_or_else(|| Fault::invalid("connection result has no profile"))?;
        value["data"]["credentials_required"] =
            json!(status.get(&id).ok_or_else(|| Fault::new(
                ErrorCode::PermissionDenied,
                "connection result is outside the captured turn"
            ))?);
    }
    Ok(())
}
