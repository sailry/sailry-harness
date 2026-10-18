use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{CredentialId, ProviderId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Authentication {
    ApiKey,
    /// Official Sign in with ChatGPT registration and public API access.
    ChatGpt,
    /// Codex device authorization and its account-scoped backend.
    #[serde(rename = "chat_gpt_oauth")]
    ChatGptOAuth,
    /// Pi-compatible Claude subscription authorization.
    #[serde(rename = "claude_oauth")]
    ClaudeOAuth,
    Copilot,
    /// A Node-owned login flow declared by the selected channel package.
    Plugin,
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
    /// Verified provider identity for display, never token or plan metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
    pub revision: u64,
    pub expires_at_ms: Option<u64>,
    pub revoked: bool,
}
