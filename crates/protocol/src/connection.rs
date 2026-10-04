//! Agent access to Node-owned connections never includes credentials.
use crate::{DatabaseId, ProjectId, SshId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Resource {
    Database(DatabaseId),
    Ssh(SshId),
}

/// No sharing policy means private, available only in a connection-bound session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", content = "projects", rename_all = "snake_case")]
pub enum Sharing {
    Global,
    Projects(Vec<ProjectId>),
}

impl Sharing {
    pub fn includes(&self, project: ProjectId) -> bool {
        match self {
            Self::Global => true,
            Self::Projects(projects) => projects.contains(&project),
        }
    }
}
