use super::*;
use futures::TryStreamExt;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{
    Column, ConnectOptions as _, Connection as _, Either, Executor, Row, SqlSafeStr, Statement,
    ValueRef,
};

pub(super) async fn run(
    operation: &Operation,
    query: &Query,
    tunnel_port: Option<u16>,
) -> Result<ResultSet, Fault> {
    let sailry_protocol::database::Server {
        host,
        port,
        database,
        username,
        tls: mode,
        socket,
    } = operation
        .profile
        .connection
        .server()
        .ok_or_else(|| Fault::invalid("database connection requires a server"))?;
    let mut options = PgConnectOptions::new_without_pgpass()
        .host(host)
        .port(tunnel_port.unwrap_or(port))
        .database(if database.is_empty() {
            "postgres"
        } else {
            database
        })
        .username(username)
        .password(operation.password.expose())
        .ssl_mode(match mode {
            Tls::Disable => PgSslMode::Disable,
            Tls::Prefer => PgSslMode::Prefer,
            Tls::Require => PgSslMode::VerifyFull,
        })
        .ssl_verify_server(true)
        .disable_statement_logging();
    if let Some(socket) = socket {
        options = options.socket(socket);
    }
    if tunnel_port.is_some() {
        options = options.connect_addr(std::net::Ipv4Addr::LOCALHOST.into());
    }
    if let Some(roots) = tls::roots() {
        options = options.ssl_root_cert_from_pem(roots.to_vec());
    }
    if operation.profile.read_only {
        options = options.options([("default_transaction_read_only", "on")]);
    }
    let mut connection = PgConnection::connect_with(&options)
        .await
        .map_err(|error| connection_error(error, &operation.password))?;
    // Preparation rejects multiple statements; execution through &str retains
    // PostgreSQL's text representation for arbitrary and extension-defined types.
    // The command is the user's complete SQL statement, admitted under its captured permissions.
    let sql = sqlx::AssertSqlSafe(query.sql.clone()).into_sql_str();
    let statement = connection.prepare(sql.clone()).await.map_err(query_error)?;
    let mut result = Rows::new(
        statement
            .columns()
            .iter()
            .map(|column| column.name().into())
            .collect(),
        query.row_limit,
    )?;
    let mut stream = connection.fetch_many(sql);
    while let Some(message) = stream.try_next().await.map_err(execution_error)? {
        match message {
            Either::Left(done) if result.result.columns.is_empty() => {
                result.result.affected_rows = done.rows_affected()
            }
            Either::Right(row) => {
                result.push(
                    (0..row.len())
                        .map(|index| {
                            let value = row.try_get_raw(index).map_err(query_error)?;
                            if value.is_null() {
                                Ok(Value::Null)
                            } else {
                                value
                                    .as_str()
                                    .map(|text| Value::Text(text.into()))
                                    .map_err(query_error)
                            }
                        })
                        .collect::<Result<Vec<_>, Fault>>()?,
                )?;
            }
            _ => {}
        }
    }
    Ok(result.result)
}
