//! Private stored captures retain their original credential data, never execution APIs.
use sailry_protocol::{CredentialId, Secret};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct McpGrant {
    pub id: CredentialId,
    pub endpoint: String,
    pub revision: u64,
    pub credentials: Option<Secret>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Identity {
    pub login: String,
    pub source: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Authorized {
    pub id: CredentialId,
    pub revision: u64,
    pub grant: Option<Grant>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Grant {
    pub declaration: Declaration,
    pub configuration: Configuration,
    pub access_token: Secret,
    pub refresh_token: Option<Secret>,
    pub expires_at_ms: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Declaration {
    pub provider: String,
    pub scopes: Vec<String>,
    pub credentials: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(in crate::store) struct Configuration {
    pub client_id: String,
    pub client_secret: Option<Secret>,
    pub tenant: Option<String>,
}
