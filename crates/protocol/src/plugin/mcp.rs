//! MCP declarations and execution Node configuration use the standard document shape.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

pub const SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json";

pub(crate) fn default_allowed() -> bool {
    true
}

/// Static configuration availability, never proof that a server is running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    #[default]
    Available,
    PackageDisabled,
    ServerDisabled,
    ConfigurationRequired,
    ConnectionRequired,
    ConnectionDisabled,
    AuthorizationExpired,
    ConsentRequired,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Runtime {
    pub state: RuntimeState,
    pub tools: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<crate::Fault>,
    pub updated_at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Connecting,
    Connected,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub package: super::Reference,
    pub server: String,
    pub tools: Vec<Tool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
}

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
        /// Server tool names exposed to agents; others are never registered.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tools: Option<Vec<String>>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tools: Option<Vec<String>>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tools: Option<Vec<String>>,
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

    /// The declared tool selection; `None` exposes every listed tool.
    pub fn tools(&self) -> Option<&[String]> {
        let (Self::Stdio { tools, .. } | Self::Http { tools, .. } | Self::Sse { tools, .. }) = self;
        tools.as_deref()
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
    fn preserves_optional_runtime() {
        let original =
            json!({"name":"service","transport":"stdio","allowed":true,"availability":"available"});
        let mut server: super::super::Mcp = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(server.runtime, None);
        assert_eq!(serde_json::to_value(&server).unwrap(), original);
        server.runtime = Some(Runtime {
            state: RuntimeState::Connected,
            tools: 4,
            fault: None,
            updated_at_ms: 42,
        });
        let encoded = serde_json::to_value(&server).unwrap();
        assert_eq!(encoded["runtime"]["state"], "connected");
        assert_eq!(encoded["runtime"]["tools"], 4);
        assert_eq!(
            serde_json::from_value::<super::super::Mcp>(encoded).unwrap(),
            server
        );
        server.runtime.as_mut().unwrap().state = RuntimeState::Failed;
        server.runtime.as_mut().unwrap().fault = Some(crate::Fault::new(
            crate::ErrorCode::Unavailable,
            "MCP server is unavailable",
        ));
        assert_eq!(
            serde_json::from_slice::<super::super::Mcp>(&serde_json::to_vec(&server).unwrap())
                .unwrap(),
            server,
        );
    }

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "defaults_server_availability",
                (|scenario: &'static str| {
                    let server: super::super::Mcp = serde_json::from_value(json!({
                        "name": "service", "transport": "stdio"
                    }))
                    .unwrap();
                    assert!(server.allowed, "{scenario}");
                    assert_eq!(server.availability, Availability::Available, "{scenario}");
                    let mut value = serde_json::to_value(&server).unwrap();
                    value["allowed"] = false.into();
                    value["availability"] = "server_disabled".into();
                    let disabled: super::super::Mcp =
                        serde_json::from_value(value.clone()).unwrap();
                    assert!(!disabled.allowed, "{scenario}");
                    assert_eq!(
                        disabled.availability,
                        Availability::ServerDisabled,
                        "{scenario}"
                    );
                    assert_eq!(serde_json::to_value(disabled).unwrap(), value, "{scenario}");
                }) as fn(&'static str),
            ),
            (
                "preserves_optional_policies",
                (|scenario: &'static str| {
                    for mut value in [
                        json!({"type":"stdio","command":"${SAILRY_COMPUTER_USE}"}),
                        json!({"type":"streamable-http","url":"https://example.test/mcp"}),
                        json!({"type":"sse","url":"https://example.test/events"}),
                    ] {
                        let server: Server = serde_json::from_value(value.clone()).unwrap();
                        assert_eq!(server.options(), Options::default(), "{scenario}");
                        assert_eq!(server.tools(), None, "{scenario}");
                        value["exclusive"] = true.into();
                        value["keep_images"] = 3.into();
                        value["reuse"] = false.into();
                        value["tools"] = json!(["snapshot", "click"]);
                        let server: Server = serde_json::from_value(value.clone()).unwrap();
                        assert_eq!(
                            server.tools(),
                            Some(["snapshot".to_owned(), "click".to_owned()].as_slice()),
                            "{scenario}"
                        );
                        assert_eq!(
                            server.options(),
                            Options {
                                exclusive: true,
                                keep_images: Some(3),
                                reuse: false
                            },
                            "{scenario}"
                        );
                        let encoded = serde_json::to_value(&server).unwrap();
                        assert_eq!(
                            serde_json::from_value::<Server>(encoded).unwrap(),
                            server,
                            "{scenario}"
                        );
                        value["keep_images"] = (-1).into();
                        assert!(
                            serde_json::from_value::<Server>(value).is_err(),
                            "{scenario}"
                        );
                    }
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
