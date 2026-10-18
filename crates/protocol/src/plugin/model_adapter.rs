//! Package-owned pure translations for Node-owned model transports.
use serde::{Deserialize, Serialize};

pub mod wire;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub adapter: String,
    #[serde(default)]
    pub capabilities: Capabilities,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub entry: String,
    #[serde(default)]
    pub resources: Vec<String>,
}

impl Script {
    pub fn valid(&self) -> bool {
        resource(&self.entry)
            && self.resources.iter().all(|path| resource(path))
            && self
                .resources
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.resources.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub id: String,
    pub script: Script,
    pub origins: Vec<String>,
    pub framing: Framing,
    #[serde(default)]
    pub schema_dialect: Dialect,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub discovery: bool,
}

impl Declaration {
    pub fn valid(&self) -> bool {
        identifier(&self.id)
            && self.script.valid()
            && self.origins.iter().all(|origin| {
                url::Url::parse(origin).is_ok_and(|url| {
                    url.scheme() == "https"
                        && url.host_str().is_some()
                        && url.username().is_empty()
                        && url.password().is_none()
                        && url.path() == "/"
                        && url.query().is_none()
                        && url.fragment().is_none()
                })
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Framing {
    Sse,
    Ndjson,
    Json,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    #[default]
    Generic,
    OpenAi,
    Gemini,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    #[serde(default)]
    pub vision: bool,
    #[serde(default)]
    pub files: bool,
    #[serde(default)]
    pub tools: bool,
    #[serde(default)]
    pub reasoning: bool,
}

pub(super) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

fn resource(path: &str) -> bool {
    path.starts_with("dev.sailry.platform/")
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
