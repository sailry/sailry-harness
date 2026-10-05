use super::ModelApi;
use crate::{CredentialRef, Effort, ProviderId, Secret};
use serde::{Deserialize, Serialize};
mod configuration;

/// Discovery uses either a draft API-key connection or a saved Node revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Source {
    Draft(Draft),
    Saved {
        provider: ProviderId,
        expected_revision: u64,
    },
}

/// Draft queries never persist credentials or configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub provider: ProviderId,
    pub api: ModelApi,
    pub endpoint: String,
    pub credential: Option<CredentialRef>,
    pub secret: Option<Secret>,
}

/// A verified model catalog and the base endpoint used to retrieve it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub endpoint: String,
    pub models: Vec<Model>,
}

/// Native model metadata; missing fields remain unknown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub context: Option<u32>,
    pub output: Option<u32>,
    pub capabilities: Option<Capabilities>,
}

/// Missing fields are unknown; only explicit provider metadata is represented.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub vision: Option<bool>,
    pub tools: Option<bool>,
    pub web_search: Option<bool>,
    pub reasoning: Option<bool>,
    pub efforts: Option<Vec<Effort>>,
    pub default_effort: Option<Effort>,
}

/// Checks model availability and declared limits, without running a model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validation {
    pub provider: ProviderId,
    pub revision: u64,
    pub missing: Vec<String>,
    pub exceeded: Vec<String>,
    pub unverified: Vec<String>,
}
