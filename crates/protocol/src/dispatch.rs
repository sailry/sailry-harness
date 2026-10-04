//! Node-owned event delivery, schedules and grouped callback execution.
use crate::{EventId, Fault, JobId, RequestId, ScheduleId, SessionId, WorktreeId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_ITEMS: usize = 256;
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_PENDING: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub package: String,
    pub topic: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub source: Source,
    pub payload: serde_json::Value,
    pub timestamp_ms: i64,
    pub schedule: Option<ScheduleId>,
    pub scheduled_ms: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub worktree: Option<WorktreeId>,
    pub session: Option<SessionId>,
}

/// Callbacks use the same admitted host commands as direct software actions.
/// Explicit JSON pointers bind event data without an expression language.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Callback {
    pub scope: Scope,
    pub completion: Completion,
    pub command: Box<crate::Command>,
    /// Destination pointer in the command -> source pointer in the event.
    pub bindings: BTreeMap<String, String>,
}

/// A turn callback completes after approvals and model/tool work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completion {
    Command,
    Turn,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handler {
    pub name: String,
    pub revision: u64,
    pub enabled: bool,
    pub source: Source,
    pub queue: String,
    pub callback: Callback,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Timing {
    Once {
        at_ms: i64,
    },
    /// Fixed elapsed interval. Missed ticks coalesce to the latest due tick.
    Every {
        anchor_ms: i64,
        interval_ms: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub id: ScheduleId,
    pub revision: u64,
    pub enabled: bool,
    pub topic: String,
    pub payload: serde_json::Value,
    pub timing: Timing,
    /// Derived by Node; callers cannot choose a cursor independently of timing.
    pub next_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub event: Event,
    pub handler: String,
    pub queue: String,
    pub request: RequestId,
    pub status: Status,
    pub created_ms: i64,
    pub started_ms: Option<i64>,
    pub finished_ms: Option<i64>,
    pub error: Option<Fault>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Command {
    ListHandlers,
    SaveHandler(Handler),
    RemoveHandler {
        name: String,
        expected_revision: u64,
        /// Cancel only unstarted deliveries; running effects keep their native lifecycle.
        #[serde(default)]
        cancel_pending: bool,
    },
    ListSchedules,
    SaveSchedule(Schedule),
    RemoveSchedule {
        id: ScheduleId,
        expected_revision: u64,
    },
    Publish {
        topic: String,
        payload: serde_json::Value,
    },
    /// Direct jobs use a registered handler and the same queue as event delivery.
    Enqueue {
        handler: String,
        payload: serde_json::Value,
    },
    ListJobs {
        before: Option<u64>,
        limit: u16,
    },
    ReadJob {
        id: JobId,
    },
    /// Reads this package's original callback receipt, not a second execution ledger.
    ReadResult {
        id: JobId,
    },
    /// Only unstarted jobs can be cancelled. Running effects require their native action.
    CancelJob {
        id: JobId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub jobs: Vec<Job>,
    pub next_before: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Output {
    Handlers(Vec<Handler>),
    Handler(Handler),
    Schedules(Vec<Schedule>),
    Schedule(Schedule),
    Published {
        event: EventId,
        jobs: Vec<JobId>,
    },
    Job(Job),
    Jobs(Page),
    Result(crate::RequestOutcome),
    Removed,
    #[serde(other, deserialize_with = "crate::message::discard_payload")]
    Unsupported,
}

impl Output {
    pub fn supported(&self) -> bool {
        match self {
            Self::Unsupported => false,
            Self::Result(outcome) => outcome.supported(),
            _ => true,
        }
    }
}
