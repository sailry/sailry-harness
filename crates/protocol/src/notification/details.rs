//! Optional notice content; execution references are authored by their Node owner.
use crate::{ErrorCode, TerminalId, TurnId, conversation};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Details {
    Diagnostic(Diagnostic),
    Task(Task),
    Terminal(Terminal),
    Text { body: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub event: String,
    pub sequence: u64,
    pub turn: TurnId,
    pub cause: Cause,
    pub version: String,
    pub platform: String,
}

/// Stable categories exclude provider bodies, credentials, and conversation content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Cause {
    Compaction(conversation::compaction::Failure),
    Execution { code: ErrorCode },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub turn: TurnId,
    pub status: conversation::Status,
    pub started_ms: Option<i64>,
    pub finished_ms: Option<i64>,
    pub error: Option<ErrorCode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terminal {
    pub id: TerminalId,
    pub code: Option<u32>,
    pub failed: bool,
}
