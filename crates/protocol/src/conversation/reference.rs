//! Context selections are scoped to the owning turn's execution Node and worktree.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Target {
    Ssh(crate::SshId),
    Database {
        connection: crate::DatabaseId,
        database: Option<String>,
        table: Option<crate::database::Table>,
    },
    File(String),
    Directory(String),
    Agent(crate::role::Reference),
    Session(crate::SessionId),
    /// Explicit capability selection; versions are captured by turn admission.
    Plugin(String),
    Skill {
        package: String,
        name: String,
    },
    Host,
    Project,
    Worktree,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub target: Target,
    pub label: String,
}

impl super::Input {
    pub fn selected_role(&self) -> Option<crate::role::Reference> {
        self.references
            .iter()
            .find_map(|reference| match reference.target {
                Target::Agent(role) => Some(role),
                _ => None,
            })
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.attachments.is_empty() && self.references.is_empty()
    }
}
