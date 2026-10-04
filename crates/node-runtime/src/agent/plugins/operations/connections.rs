//! Connection operations retain the admitted choices and existing Node commands.
use super::*;
use crate::store::agent::connections::Action;
use sailry_protocol::connection::Resource;
use serde::Deserialize;

pub(super) fn handles(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::BrowseDatabase
            | Operation::QueryDatabase
            | Operation::ExecuteDatabase
            | Operation::RunSsh
            | Operation::TransferSsh
    )
}

pub(super) fn description(operation: Operation) -> &'static str {
    match operation {
        Operation::BrowseDatabase => "Read a catalog from an admitted database connection",
        Operation::QueryDatabase => "Query an admitted database through a read-only connection",
        Operation::ExecuteDatabase => "Execute SQL on an admitted database connection",
        Operation::RunSsh => "Run a command through an admitted SSH connection",
        Operation::TransferSsh => "Transfer a file through an admitted SSH connection",
        _ => unreachable!(),
    }
}

pub(super) fn parameters(operation: Operation) -> Value {
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

pub(super) fn action(operation: Operation, arguments: Value) -> Result<(Resource, Action), Fault> {
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
        serde_json::from_value(value).map_err(|error| {
            Fault::new(
                ErrorCode::InvalidRequest,
                format!("invalid connection operation: {error}"),
            )
        })
    }
    Ok(match operation {
        Operation::BrowseDatabase => {
            let input: Catalog = parse(arguments)?;
            (
                Resource::Database(input.connection),
                Action::Catalog {
                    database: input.database,
                },
            )
        }
        Operation::QueryDatabase | Operation::ExecuteDatabase => {
            let input: Query = parse(arguments)?;
            (
                Resource::Database(input.connection),
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
                Resource::Ssh(input.connection),
                Action::Run {
                    command: input.command,
                    timeout_ms: Some(input.timeout_ms),
                },
            )
        }
        Operation::TransferSsh => {
            let input: Transfer = parse(arguments)?;
            (
                Resource::Ssh(input.connection),
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
