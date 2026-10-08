use super::*;
use futures::TryStreamExt;
use tokio_postgres::{SimpleQueryMessage, config::SslMode};
use tokio_postgres_rustls::MakeRustlsConnect;

struct Driver(tokio::task::JoinHandle<()>);
impl Drop for Driver {
    fn drop(&mut self) {
        self.0.abort();
    }
}

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
        tls,
        socket,
    } = operation
        .profile
        .connection
        .server()
        .ok_or_else(|| Fault::invalid("database connection requires a server"))?;
    let mut config = tokio_postgres::Config::new();
    config
        .host(socket.unwrap_or(host))
        .port(tunnel_port.unwrap_or(port))
        .dbname(if database.is_empty() {
            "postgres"
        } else {
            database
        })
        .user(username)
        .password(operation.password.expose());
    if tunnel_port.is_some() {
        config.hostaddr(std::net::Ipv4Addr::LOCALHOST.into());
    }
    config.ssl_mode(match tls {
        Tls::Disable => SslMode::Disable,
        Tls::Prefer => SslMode::Prefer,
        Tls::Require => SslMode::Require,
    });
    if operation.profile.read_only {
        config.options("-c default_transaction_read_only=on");
    }
    let (client, connection) = config
        .connect(MakeRustlsConnect::new(tls::postgres()))
        .await
        .map_err(|_| unavailable())?;
    let _driver = Driver(tokio::spawn(async move {
        let _ = connection.await;
    }));
    // Parse before using the text protocol: PostgreSQL rejects multiple statements
    // here, while simple-query rows preserve arbitrary SQL types as server text.
    let _statement = client.prepare(&query.sql).await.map_err(query_error)?;
    let stream = client
        .simple_query_raw(&query.sql)
        .await
        .map_err(execution_error)?;
    tokio::pin!(stream);
    let mut result = Rows::new(Vec::new(), query.row_limit)?;
    while let Some(message) = stream.try_next().await.map_err(execution_error)? {
        match message {
            SimpleQueryMessage::RowDescription(columns) => {
                result = Rows::new(
                    columns.iter().map(|column| column.name().into()).collect(),
                    query.row_limit,
                )?;
            }
            SimpleQueryMessage::Row(row) => {
                result.push(
                    (0..row.len())
                        .map(|index| {
                            row.get(index)
                                .map_or(Value::Null, |value| Value::Text(value.into()))
                        })
                        .collect(),
                )?;
            }
            SimpleQueryMessage::CommandComplete(count) if result.result.columns.is_empty() => {
                result.result.affected_rows = count
            }
            _ => {}
        }
    }
    Ok(result.result)
}
fn execution_error(error: tokio_postgres::Error) -> Fault {
    if error.as_db_error().is_some() {
        query_error(error)
    } else {
        unknown()
    }
}
