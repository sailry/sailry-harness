use super::*;
use mysql_async::{Conn, DriverError, OptsBuilder, Value as SqlValue, prelude::Queryable};

pub(super) async fn run(
    operation: &Operation,
    query: &Query,
    tunnel_port: Option<u16>,
) -> Result<ResultSet, Fault> {
    let (host, port, database, username, tls, socket) = match &operation.profile.connection {
        Connection::Network {
            host,
            port,
            database,
            username,
            tls,
            ..
        }
        | Connection::Ssh {
            host,
            port,
            database,
            username,
            tls,
            ..
        } => (host.as_str(), *port, database, username, *tls, None),
        Connection::Socket {
            path,
            port,
            database,
            username,
            ..
        } => (
            "localhost",
            *port,
            database,
            username,
            Tls::Disable,
            Some(path),
        ),
        _ => unreachable!(),
    };
    // Keep localhost on one endpoint across catalog and table requests. An explicit
    // IPv6 address still selects IPv6; other hostnames retain normal DNS resolution.
    let host = if tunnel_port.is_none() && host.eq_ignore_ascii_case("localhost") {
        "127.0.0.1"
    } else {
        host
    };
    let plain = OptsBuilder::default()
        .ip_or_hostname(host)
        .tcp_port(tunnel_port.unwrap_or(port))
        .socket(socket.cloned())
        .resolved_ips(tunnel_port.map(|_| vec![std::net::Ipv4Addr::LOCALHOST.into()]))
        // A server's advertised socket may belong to a different local instance
        // when TCP reaches a container or SSH tunnel. Honor the network profile.
        .prefer_socket(false)
        .db_name((!database.is_empty()).then(|| database.clone()))
        .user(Some(username.clone()))
        .pass(Some(operation.password.expose().to_owned()));
    let connection_error = |error: mysql_async::Error| {
        let mut message = format!("Database connection failed: {error}");
        let password = operation.password.expose();
        if !password.is_empty() {
            message = message.replace(password, "[redacted]");
        }
        Fault::new(ErrorCode::Unavailable, message)
    };
    let mut connection = if tls == Tls::Disable {
        Conn::new(plain).await.map_err(connection_error)?
    } else {
        match Conn::new(plain.clone().ssl_opts(tls::mysql())).await {
            Ok(connection) => connection,
            Err(mysql_async::Error::Driver(DriverError::NoClientSslFlagFromServer))
                if tls == Tls::Prefer =>
            {
                Conn::new(plain).await.map_err(connection_error)?
            }
            Err(error) => return Err(connection_error(error)),
        }
    };
    if operation.profile.read_only {
        connection
            .query_drop("SET SESSION TRANSACTION READ ONLY")
            .await
            .map_err(query_error)?;
    }
    let statement = connection.prep(&query.sql).await.map_err(query_error)?;
    let mut cursor = connection
        .exec_iter(statement, ())
        .await
        .map_err(execution_error)?;
    let columns = cursor
        .columns()
        .map(|columns| {
            columns
                .iter()
                .map(|column| column.name_str().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let mut result = Rows::new(columns, query.row_limit)?;
    result.result.affected_rows = if result.result.columns.is_empty() {
        cursor.affected_rows()
    } else {
        0
    };
    while let Some(row) = cursor.next().await.map_err(execution_error)? {
        result.push(row.unwrap().into_iter().map(value).collect())?;
    }
    drop(cursor);
    let _ = connection.disconnect().await;
    Ok(result.result)
}
fn execution_error(error: mysql_async::Error) -> Fault {
    if matches!(error, mysql_async::Error::Server(_)) {
        query_error(error)
    } else {
        unknown()
    }
}
fn value(value: SqlValue) -> Value {
    match value {
        SqlValue::NULL => Value::Null,
        SqlValue::Int(value) => Value::Integer(value),
        SqlValue::UInt(value) => i64::try_from(value)
            .map(Value::Integer)
            .unwrap_or_else(|_| Value::Text(value.to_string())),
        SqlValue::Float(value) => Value::Real(value.to_string()),
        SqlValue::Double(value) => Value::Real(value.to_string()),
        SqlValue::Bytes(bytes) => String::from_utf8(bytes)
            .map(Value::Text)
            .unwrap_or_else(|error| Value::Blob(error.into_bytes())),
        value @ (SqlValue::Date(..) | SqlValue::Time(..)) => {
            Value::Text(value.as_sql(true).trim_matches('\'').into())
        }
    }
}
