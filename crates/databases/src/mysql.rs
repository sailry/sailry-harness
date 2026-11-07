use super::*;
use futures::TryStreamExt;
use sqlx::mysql::{MySqlConnectOptions, MySqlConnection, MySqlRow, MySqlSslMode};
use sqlx::{
    Column, ConnectOptions as _, Connection as _, Either, Executor, Row, SqlSafeStr, TypeInfo,
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
    let mut options = MySqlConnectOptions::new()
        .host(host)
        .port(tunnel_port.unwrap_or(port))
        .username(username)
        .password(operation.password.expose())
        .timezone(None)
        .pipes_as_concat(false)
        .no_engine_substitution(false)
        .ssl_mode(match mode {
            Tls::Disable => MySqlSslMode::Disabled,
            Tls::Prefer => MySqlSslMode::Preferred,
            Tls::Require => MySqlSslMode::VerifyIdentity,
        })
        .ssl_verify_server(true)
        .disable_statement_logging();
    if !database.is_empty() {
        options = options.database(database);
    }
    if let Some(socket) = socket {
        options = options.socket(socket);
    }
    if tunnel_port.is_some() || (socket.is_none() && host.eq_ignore_ascii_case("localhost")) {
        options = options.connect_addr(std::net::Ipv4Addr::LOCALHOST.into());
    }
    if let Some(roots) = tls::roots() {
        options = options.ssl_ca_from_pem(roots.to_vec());
    }
    let mut connection = MySqlConnection::connect_with(&options)
        .await
        .map_err(|error| connection_error(error, &operation.password))?;
    if operation.profile.read_only {
        connection
            .execute("SET SESSION TRANSACTION READ ONLY")
            .await
            .map_err(query_error)?;
    }
    // The command is the user's complete SQL statement, admitted under its captured permissions.
    let sql = sqlx::AssertSqlSafe(query.sql.clone()).into_sql_str();
    let _statement = connection.prepare(sql.clone()).await.map_err(query_error)?;
    let mut result = Rows::new(Vec::new(), query.row_limit)?;
    // CALL can describe no columns during preparation and return several sets.
    // Keep its first set's metadata and rows, then drain the remainder without
    // changing that result or its affected-row count.
    let mut first = true;
    let mut stream = connection.fetch_many(sql);
    while let Some(message) = stream.try_next().await.map_err(execution_error)? {
        match message {
            Either::Left(done) if first => {
                if result.result.columns.is_empty() {
                    result = Rows::new(
                        done.columns()
                            .iter()
                            .map(|column| column.name().into())
                            .collect(),
                        query.row_limit,
                    )?;
                }
                if result.result.columns.is_empty() {
                    result.result.affected_rows = done.rows_affected();
                }
                first = false;
            }
            Either::Right(row) if first => {
                if result.result.columns.is_empty() {
                    result = Rows::new(
                        row.columns()
                            .iter()
                            .map(|column| column.name().into())
                            .collect(),
                        query.row_limit,
                    )?;
                }
                result.push(
                    (0..row.len())
                        .map(|index| value(&row, index))
                        .collect::<Result<Vec<_>, _>>()?,
                )?;
            }
            _ => {}
        }
    }
    Ok(result.result)
}

fn value(row: &MySqlRow, index: usize) -> Result<Value, Fault> {
    let raw = row.try_get_raw(index).map_err(query_error)?;
    if raw.is_null() {
        return Ok(Value::Null);
    }
    let bytes: Vec<u8> = row.try_get_unchecked(index).map_err(query_error)?;
    let kind = raw.type_info();
    let kind = kind.name();
    if matches!(
        kind.split(' ').next(),
        Some("BOOLEAN" | "TINYINT" | "SMALLINT" | "INT" | "MEDIUMINT" | "BIGINT" | "YEAR")
    ) {
        let text = std::str::from_utf8(&bytes).map_err(query_error)?;
        return Ok(text
            .parse()
            .map(Value::Integer)
            .unwrap_or_else(|_| Value::Text(text.into())));
    }
    if matches!(kind, "FLOAT" | "DOUBLE") {
        return Ok(Value::Real(String::from_utf8(bytes).map_err(query_error)?));
    }
    Ok(String::from_utf8(bytes)
        .map(Value::Text)
        .unwrap_or_else(|error| Value::Blob(error.into_bytes())))
}
