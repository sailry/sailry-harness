//! Standalone Agent Skills imported into the execution Node's package inventory.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub repository: String,
    pub git_ref: Option<String>,
    pub path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    pub repository: String,
    pub git_ref: String,
    pub commit: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Stable inventory identity for this repository and skill directory.
    pub name: String,
    pub path: String,
    pub skill: super::Skill,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovery {
    pub source: Resolved,
    pub skills: Vec<Candidate>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: Resolved,
    pub path: String,
}
