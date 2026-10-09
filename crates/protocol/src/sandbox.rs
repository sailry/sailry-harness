//! Node-owned defaults for subsequent Agent commands in every session.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 0,
            enabled: true,
        }
    }
}
