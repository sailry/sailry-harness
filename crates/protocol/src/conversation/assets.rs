//! Read-only inventory derived from committed conversation history.
use crate::{SessionId, TurnId, WorktreeId};
use serde::{Deserialize, Serialize};

pub const MAX_GROUPS: u16 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Resource,
    Artifact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    pub kind: Option<Kind>,
    pub before: Option<u64>,
    pub limit: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Target {
    Attachment(crate::attachment::Attachment),
    Image(super::Image),
    /// Relative to the originating turn's worktree; opening rechecks file access.
    File {
        path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub sequence: u64,
    pub turn: TurnId,
    pub worktree: WorktreeId,
    pub timestamp_ms: i64,
    pub kind: Kind,
    pub items: Vec<Target>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub session: SessionId,
    pub revision: u64,
    /// Newest committed event first. Repeated targets within an event appear once.
    pub groups: Vec<Group>,
    pub next_before: Option<u64>,
}
