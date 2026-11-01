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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub id: String,
    pub label: crate::plugin::desktop::Navigation,
    pub selection: Selection,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub presets: Vec<Preset>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listing {
    pub presets: Vec<Preset>,
    pub unavailable: bool,
}

/// HTTPS service response time, separate from a paired connection's RTT.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Latency {
    pub selection: Selection,
    /// No value means no successful response was measured.
    pub millis: Option<u64>,
}
