use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{CredentialId, ProviderId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authentication {
    ApiKey,
    /// Resolve the execution Node's cloud identity; never a controller credential.
    Host,
    ChatGpt,
    Copilot,
}

/// Private credential value for writes and explicit, revision-bound settings reads.
/// Never include this type in events or snapshots.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: String) -> Self {
        Self(value)
    }
    /// Expose only in credential storage, execution, or the requested settings editor.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret([REDACTED])")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credential {
    pub id: CredentialId,
    pub provider: ProviderId,
    pub authentication: Authentication,
    pub revision: u64,
    pub expires_at_ms: Option<u64>,
    pub revoked: bool,
}
