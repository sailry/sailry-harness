//! Durable completion evidence for work owned by one conversation.
use crate::{RequestId, SessionId, TurnId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Kind {
    /// The exact completed execution, including a final guidance handoff.
    Agent {
        session: SessionId,
        turn: TurnId,
    },
    Command {
        id: RequestId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    Failed,
    Cancelled,
    Stopped,
    TimedOut,
    Interrupted,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Running,
    Pending,
    Delivered,
    Consumed,
    Dismissed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    /// Namespaced source identity, independent of a reusable child session.
    pub id: String,
    pub session: SessionId,
    pub turn: TurnId,
    pub kind: Kind,
    pub title: String,
    pub revision: u64,
    pub state: State,
    pub outcome: Option<Outcome>,
    pub started_ms: i64,
    pub finished_ms: Option<i64>,
    pub summary: Option<String>,
    /// A reservation while pending; delivery is confirmed by canonical input.
    pub delivery: Option<TurnId>,
    pub recovered: bool,
    pub suppressed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    pub outcome: Outcome,
    pub summary: Option<String>,
}

#[cfg(test)]
mod tests;
