//! Read-only update inspection; installation remains an explicit durable command.
use super::{Info, Summary, catalog};
use crate::Fault;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub name: String,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "package", rename_all = "snake_case")]
pub enum Candidate {
    Package(Info),
    /// Repository inspection is deferred until an explicit update.
    Official(catalog::Entry),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub current: Summary,
    pub available: Option<Candidate>,
    /// Uploaded packages without a reusable source require another explicit selection.
    pub manual: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batch {
    pub reports: Vec<Report>,
    pub failures: Vec<Failure>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub name: String,
    pub error: Fault,
}
