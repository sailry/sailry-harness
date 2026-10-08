use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    Sqlite,
    Mysql,
    Postgres,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tls {
    Disable,
    Prefer,
    Require,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Connection {
    Sqlite {
        path: String,
    },
    Socket {
        engine: Engine,
        path: String,
        port: u16,
        database: String,
        username: String,
    },
    Ssh {
        engine: Engine,
        host: String,
        port: u16,
        database: String,
        username: String,
        tls: Tls,
        ssh: crate::SshId,
    },
    Network {
        engine: Engine,
        host: String,
        port: u16,
        database: String,
        username: String,
        tls: Tls,
    },
}

/// Borrowed connection options for network and socket database drivers.
pub struct Server<'a> {
    pub host: &'a str,
    pub port: u16,
    pub database: &'a str,
    pub username: &'a str,
    pub tls: Tls,
    pub socket: Option<&'a str>,
}

impl Connection {
    pub fn server(&self) -> Option<Server<'_>> {
        match self {
            Self::Network {
                host,
                port,
                database,
                username,
                tls,
                ..
            }
            | Self::Ssh {
                host,
                port,
                database,
                username,
                tls,
                ..
            } => Some(Server {
                host,
                port: *port,
                database,
                username,
                tls: *tls,
                socket: None,
            }),
            Self::Socket {
                path,
                port,
                database,
                username,
                ..
            } => Some(Server {
                host: "localhost",
                port: *port,
                database,
                username,
                tls: Tls::Disable,
                socket: Some(path),
            }),
            Self::Sqlite { .. } => None,
        }
    }

    pub fn engine(&self) -> Engine {
        match self {
            Self::Sqlite { .. } => Engine::Sqlite,
            Self::Network { engine, .. }
            | Self::Socket { engine, .. }
            | Self::Ssh { engine, .. } => *engine,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharing: Option<crate::connection::Sharing>,
    pub id: crate::DatabaseId,
    pub revision: u64,
    pub name: String,
    pub connection: Connection,
    pub read_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Value {
    Null,
    Integer(i64),
    Real(String),
    Text(String),
    Blob(Vec<u8>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultSet {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub affected_rows: u64,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Outcome {
    Connected,
    Query(ResultSet),
    Catalog(Catalog),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub schema: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Catalog {
    Databases(Vec<String>),
    Tables {
        database: String,
        tables: Vec<Table>,
    },
}

#[cfg(test)]
mod server_options {
    use super::*;

    #[test]
    fn preserves_network_and_tunnel_options() {
        for connection in [
            Connection::Network {
                engine: Engine::Postgres,
                host: "database.internal".into(),
                port: 5432,
                database: "project".into(),
                username: "reader".into(),
                tls: Tls::Require,
            },
            Connection::Ssh {
                engine: Engine::Postgres,
                host: "database.internal".into(),
                port: 5432,
                database: "project".into(),
                username: "reader".into(),
                tls: Tls::Require,
                ssh: crate::SshId::new(),
            },
        ] {
            let options = connection.server().unwrap();
            assert_eq!(options.host, "database.internal");
            assert_eq!(options.port, 5432);
            assert_eq!(options.database, "project");
            assert_eq!(options.username, "reader");
            assert_eq!(options.tls, Tls::Require);
            assert!(options.socket.is_none());
        }
    }

    #[test]
    fn socket_disables_network_tls() {
        let connection = Connection::Socket {
            engine: Engine::Mysql,
            path: "/tmp/database.sock".into(),
            port: 3306,
            database: "project".into(),
            username: "reader".into(),
        };
        let options = connection.server().unwrap();
        assert_eq!(options.host, "localhost");
        assert_eq!(options.port, 3306);
        assert_eq!(options.database, "project");
        assert_eq!(options.username, "reader");
        assert_eq!(options.socket, Some("/tmp/database.sock"));
        assert_eq!(options.tls, Tls::Disable);
    }

    #[test]
    fn sqlite_has_no_server_options() {
        assert!(
            Connection::Sqlite {
                path: "database.sqlite".into()
            }
            .server()
            .is_none()
        );
    }
}
