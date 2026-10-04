//! Bounded command results; process handles and execution remain on the Node.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Outcome {
    Exited(i32),
    Signal(i32),
    TimedOut,
    Cancelled,
    /// Execution or cleanup could not be confirmed; never automatically replay.
    Unknown(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capture {
    pub text: String,
    pub truncated: bool,
    pub invalid_utf8: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    pub outcome: Outcome,
    pub stdout: Capture,
    pub stderr: Capture,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Status {
    Running,
    Stopping,
    Finished(Outcome),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Info {
    pub id: crate::RequestId,
    pub session: crate::SessionId,
    pub turn: crate::TurnId,
    pub command: String,
    pub cwd: String,
    pub started_ms: u64,
    pub status: Status,
    pub services: Vec<Service>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub info: Info,
    pub stdout: Capture,
    pub stderr: Capture,
}

/// A loopback HTTP address announced by a managed command, not a port scan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Service {
    pub port: u16,
    pub url: String,
}
