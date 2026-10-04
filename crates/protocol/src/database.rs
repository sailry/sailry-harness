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

impl Connection {
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
