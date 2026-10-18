//! Execution Node relay configuration; transport addresses remain opaque to controllers.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Selection {
    Iroh,
    Custom { relays: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    /// Persisted intent. None identifies a direct-only endpoint.
    pub selection: Option<Selection>,
    /// The live endpoint's last verified relay selection.
    pub effective: Option<Selection>,
    pub address: String,
}
