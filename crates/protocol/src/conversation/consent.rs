//! Provider-scoped approval for package access, distinct from package enablement.
use crate::{WorktreeId, plugin::Reference};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scopes {
    #[serde(default)]
    pub messages: bool,
    #[serde(default)]
    pub credentials: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    /// The exact package displayed when these scopes were approved.
    pub package: Reference,
    pub scopes: Scopes,
    /// Stamped by the execution Node after resolving the current installation.
    #[serde(default)]
    pub origin: Option<Origin>,
}

/// Installation coordinates, not an implementation version or a trust badge.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Origin {
    Bundled,
    Git {
        repository: String,
        path: String,
    },
    Registry {
        namespace: String,
        slug: String,
    },
    Worktree {
        worktree: WorktreeId,
        path: String,
    },
    /// Imports without a stable authenticated source need explicit reapproval.
    Imported {
        digest: String,
    },
}

impl Origin {
    /// Public source coordinates alone do not approve changed third-party code.
    pub fn requires_digest(&self) -> bool {
        match self {
            Self::Bundled => false,
            Self::Git { repository, .. } => {
                repository.trim_end_matches(".git") != "https://github.com/sailry/sailry-plugins"
            }
            _ => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Messages,
    Credentials,
}

impl Scopes {
    pub fn allows(self, scope: Scope) -> bool {
        match scope {
            Scope::Messages => self.messages,
            Scope::Credentials => self.credentials,
        }
    }
}
