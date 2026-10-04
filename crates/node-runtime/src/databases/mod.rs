//! Database execution adapted from sailry-code 67ae9fa0 sailry-connections.
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, Secret, database::*};
use std::{path::PathBuf, time::Duration};

pub(crate) mod catalog;
mod mysql;
mod postgres;
mod sqlite;
mod tls;
mod transport;
pub(crate) use transport::Ssh;

pub(crate) struct Query {
    pub sql: String,
    pub row_limit: usize,
    pub timeout_ms: u64,
}
pub(crate) struct Operation {
    pub profile: Profile,
    pub password: Secret,
    pub ssh: Option<Ssh>,
    pub query: Option<Query>,
    pub browse: Option<Option<String>>,
    pub storage: Option<PathBuf>,
}

pub(crate) async fn execute(
    operation: Operation,
    stop: &CancellationToken,
    closed: &CancellationToken,
) -> Result<Outcome, Fault> {
    if let Some(database) = &operation.browse {
        return catalog::read(&operation, database.clone(), stop, closed).await;
    }
    let query = operation.query.as_ref().map_or(
        Query {
            sql: "SELECT 1".into(),
            row_limit: 1,
            timeout_ms: 30_000,
        },
        |query| Query {
            sql: query.sql.clone(),
            row_limit: query.row_limit,
            timeout_ms: query.timeout_ms,
        },
    );
    let check = operation.query.is_none();
    let result = if matches!(operation.profile.connection, Connection::Sqlite { .. }) {
        let stop = stop.clone();
        let closed = closed.clone();
        tokio::task::spawn_blocking(move || sqlite::run(&operation, &query, stop, closed))
            .await
            .map_err(|_| unknown())??
    } else {
        let work = async {
            let tunnel = transport::open(&operation, stop, closed).await?;
            let port = tunnel.as_ref().map(|tunnel| tunnel.port);
            match operation.profile.connection.engine() {
                Engine::Mysql => mysql::run(&operation, &query, port).await,
                Engine::Postgres => postgres::run(&operation, &query, port).await,
                _ => Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "invalid database engine",
                )),
            }
        };
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(unknown()),
            _ = closed.cancelled() => return Err(unknown()),
            result = tokio::time::timeout(Duration::from_millis(query.timeout_ms), work) => result.map_err(|_| unknown())??,
        }
    };
    Ok(if check {
        Outcome::Connected
    } else {
        Outcome::Query(result)
    })
}

struct Rows {
    result: ResultSet,
    limit: usize,
    bytes: usize,
}
impl Rows {
    fn new(columns: Vec<String>, limit: usize) -> Result<Self, Fault> {
        let bytes = serde_json::to_vec(&columns).map_err(query_error)?.len();
        if bytes > 512 * 1024 {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "database column metadata exceeds the result limit",
            ));
        }
        Ok(Self {
            result: ResultSet {
                columns,
                ..Default::default()
            },
            limit,
            bytes,
        })
    }
    fn push(&mut self, row: Vec<Value>) -> Result<(), Fault> {
        let bytes = serde_json::to_vec(&row).map_err(query_error)?.len();
        if self.result.truncated
            || self.result.rows.len() >= self.limit
            || self.bytes + bytes > 512 * 1024
        {
            self.result.truncated = true;
        } else {
            self.bytes += bytes;
            self.result.rows.push(row);
        }
        Ok(())
    }
}
fn query_error(error: impl std::fmt::Display) -> Fault {
    Fault::new(
        ErrorCode::InvalidRequest,
        format!("Database query failed: {error}"),
    )
}
fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "Database connection failed")
}
fn unknown() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "Database completion is uncertain; inspect the database before retrying",
    )
}
