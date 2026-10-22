//! Safe context-maintenance diagnostics carried by canonical conversation entries.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Automatic,
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Unknown,
    History,
    Budget,
    Request,
    Summary,
    Persistence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Unknown,
    HistoryMismatch,
    NoFittingPrefix,
    RequestFailed,
    RequestTimeout,
    RateLimited,
    AuthorizationFailed,
    ServiceRejected,
    EmptySummary,
    IncompleteResponse,
    UnexpectedOutput,
    SummaryTooLarge,
    NoReduction,
    PersistenceFailed,
}

/// Contains only stable classifications, never model input or upstream error bodies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub source: Source,
    pub stage: Stage,
    pub reason: Reason,
    pub continued: bool,
    pub status: Option<u16>,
}

impl Failure {
    pub fn resource(&self) -> serde_json::Value {
        serde_json::json!({"type":"compaction_failure", "failure":self})
    }

    pub fn from_resource(value: &serde_json::Value) -> Option<Self> {
        (value["type"] == "compaction_failure")
            .then(|| serde_json::from_value(value["failure"].clone()).ok())
            .flatten()
    }
}
