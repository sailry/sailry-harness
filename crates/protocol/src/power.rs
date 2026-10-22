//! Immediate policy on the execution Node, independent of captured turn settings.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub prevent_sleep: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 0,
            prevent_sleep: true,
        }
    }
}
