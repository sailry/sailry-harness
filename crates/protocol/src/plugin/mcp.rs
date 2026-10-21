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
        #[serde(default, skip_serializing_if = "is_false")]
        exclusive: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep_images: Option<usize>,
        #[serde(default = "default_reuse", skip_serializing_if = "is_true")]
        reuse: bool,
    },
    #[serde(rename = "streamable-http")]
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "is_false")]
        exclusive: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep_images: Option<usize>,
        #[serde(default = "default_reuse", skip_serializing_if = "is_true")]
        reuse: bool,
    },
    #[serde(rename = "sse")]
    Sse {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "is_false")]
        exclusive: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep_images: Option<usize>,
        #[serde(default = "default_reuse", skip_serializing_if = "is_true")]
        reuse: bool,
    },
}

fn default_cwd() -> String {
    "${PLUGIN_ROOT}".into()
}

fn is_false(value: &bool) -> bool {
    !value
}

fn is_true(value: &bool) -> bool {
    *value
}

fn default_reuse() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub exclusive: bool,
    pub keep_images: Option<usize>,
    pub reuse: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            exclusive: false,
            keep_images: None,
            reuse: true,
        }
    }
}

impl Server {
    pub fn options(&self) -> Options {
        let (Self::Stdio {
            exclusive,
            keep_images,
            reuse,
            ..
        }
        | Self::Http {
            exclusive,
            keep_images,
            reuse,
            ..
        }
        | Self::Sse {
            exclusive,
            keep_images,
            reuse,
            ..
        }) = self;
        Options {
            exclusive: *exclusive,
            keep_images: *keep_images,
            reuse: *reuse,
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_optional_policies() {
        for mut value in [
            json!({"type":"stdio","command":"${SAILRY_COMPUTER_USE}"}),
            json!({"type":"streamable-http","url":"https://example.test/mcp"}),
            json!({"type":"sse","url":"https://example.test/events"}),
        ] {
            let server: Server = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(server.options(), Options::default());
            value["exclusive"] = true.into();
            value["keep_images"] = 3.into();
            value["reuse"] = false.into();
            let server: Server = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(
                server.options(),
                Options {
                    exclusive: true,
                    keep_images: Some(3),
                    reuse: false
                }
            );
            let encoded = serde_json::to_value(&server).unwrap();
            assert_eq!(serde_json::from_value::<Server>(encoded).unwrap(), server);
            value["keep_images"] = (-1).into();
            assert!(serde_json::from_value::<Server>(value).is_err());
        }
    }
}
