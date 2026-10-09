//! GitHub CLI discovery exposes no account credentials.
use super::Reference;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub package: Reference,
    pub available: bool,
    pub authenticated: bool,
    #[serde(default)]
    pub bound: bool,
}
