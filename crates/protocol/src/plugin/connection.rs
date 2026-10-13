//! Presence-only connector state; values and remote reachability stay private.
use super::Reference;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub package: Reference,
    pub card: String,
    pub server: String,
    pub configuration: Configuration,
    pub authorization: Authorization,
    pub credential: Credential,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Configuration {
    Ready,
    Missing,
    Changed,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authorization {
    NotDeclared,
    Missing,
    Configured,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Credential {
    NotDeclared,
    Absent,
    Present,
}
