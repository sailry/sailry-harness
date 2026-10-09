//! Fixed-provider browser login keeps tokens on the execution Node.
use super::Reference;
use crate::{RequestId, Secret};
use serde::{Deserialize, Serialize};

pub use super::authorization::State;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Google,
    Microsoft,
    Dingtalk,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub provider: Provider,
    pub scopes: Vec<String>,
    /// Existing HTTP credential fields filled by this account's access token.
    pub credentials: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Configuration {
    pub client_id: String,
    pub client_secret: Option<Secret>,
    /// Microsoft tenant; absent selects its public multi-tenant endpoint.
    pub tenant: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub package: Reference,
    pub configured: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    pub id: RequestId,
    pub package: Reference,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Update {
    pub attempt: Attempt,
    pub revision: u64,
    pub state: State,
}
