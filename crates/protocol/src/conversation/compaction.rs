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
    TruncatedSummary,
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

/// Effective execution policy for the current or captured next model invocation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub model: String,
    pub context_limit: u32,
    pub trigger_tokens: u64,
    pub output_reserve: u32,
    pub recent_tokens: u64,
    pub summary_input: u64,
    pub summary_output: u32,
}

/// A canonical maintenance reference, independent of controller history pagination.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub sequence: u64,
    pub turn: crate::TurnId,
    pub turn_sequence: u64,
    pub timestamp_ms: i64,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Succeeded,
    Failed(Failure),
}
