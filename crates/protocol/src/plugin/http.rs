//! Explicit text HTTP requests execute on the plugin's owning Node.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_BODY: usize = 1024 * 1024;
pub const MAX_TIMEOUT_MS: u32 = 60_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
    /// A declared secret setting; its value never crosses this boundary.
    pub credential: Option<String>,
    /// A declared connection; secrets are resolved only on the execution Node.
    #[serde(default)]
    pub connector: Option<String>,
    #[serde(default)]
    pub connection: Option<super::connector::Id>,
    /// Allows one authorization recovery before a safe retry.
    #[serde(default)]
    pub idempotent: bool,
    #[serde(default = "timeout")]
    pub timeout_ms: u32,
}

fn timeout() -> u32 {
    30_000
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, Vec<String>>,
    pub body: String,
}
