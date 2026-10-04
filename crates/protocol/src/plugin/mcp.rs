//! MCP declarations and execution Node configuration use the standard document shape.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

pub const SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    #[serde(rename = "$schema")]
    pub schema: String,
    #[serde(rename = "mcpServers")]
    pub servers: BTreeMap<String, Server>,
}

impl fmt::Debug for Configuration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Configuration")
            .field("schema", &self.schema)
            .field("servers", &self.servers)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Server {
    #[serde(rename = "stdio")]
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default = "default_cwd")]
        cwd: String,
    },
    #[serde(rename = "streamable-http")]
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
    #[serde(rename = "sse")]
    Sse {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

fn default_cwd() -> String {
    "${PLUGIN_ROOT}".into()
}

impl Server {
    pub fn transport(&self) -> super::McpTransport {
        match self {
            Self::Stdio { .. } => super::McpTransport::Stdio,
            Self::Http { .. } => super::McpTransport::StreamableHttp,
            Self::Sse { .. } => super::McpTransport::Sse,
        }
    }
}

impl fmt::Debug for Server {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stdio { env, .. } => formatter
                .debug_struct("Stdio")
                .field("env", &env.keys().collect::<Vec<_>>())
                .finish_non_exhaustive(),
            Self::Http { headers, .. } | Self::Sse { headers, .. } => formatter
                .debug_struct("Http")
                .field("headers", &headers.keys().collect::<Vec<_>>())
                .finish_non_exhaustive(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub package: super::Reference,
    pub configuration: Configuration,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Definition {
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<String>,
    },
    StreamableHttp {
        url: String,
        headers: Vec<String>,
    },
    Sse {
        url: String,
        headers: Vec<String>,
    },
}

impl Definition {
    pub fn slots(&self) -> &[String] {
        match self {
            Self::Stdio { env, .. } => env,
            Self::StreamableHttp { headers, .. } | Self::Sse { headers, .. } => headers,
        }
    }

    pub fn transport(&self) -> super::McpTransport {
        match self {
            Self::Stdio { .. } => super::McpTransport::Stdio,
            Self::StreamableHttp { .. } => super::McpTransport::StreamableHttp,
            Self::Sse { .. } => super::McpTransport::Sse,
        }
    }
}
