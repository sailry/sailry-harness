//! File recovery is independent of conversation history rewinds.
use crate::{CheckpointId, RequestOutcome, SessionId, TurnId, WorktreeId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub revision: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub id: CheckpointId,
    pub turn: TurnId,
    pub worktree: WorktreeId,
    pub path: String,
    /// None means the authorized write was creating a new file.
    pub before: Option<Version>,
    pub after: Version,
    /// The original write's existing durable receipt, not a second execution status.
    pub outcome: RequestOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub session: SessionId,
    pub turn: TurnId,
    pub files: Vec<File>,
    pub next: Option<CheckpointId>,
}

/// Immutable, confirmed file-write changes belonging to one visible turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnDiff {
    pub session: SessionId,
    pub turn: TurnId,
    pub files: Vec<crate::GitDiff>,
    /// Some writes have uncertain outcomes or exceeded the response bound.
    pub partial: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Content {
    pub session: SessionId,
    pub file: File,
    pub before: Option<String>,
    /// Exact prepared content captured for the approved write.
    pub after: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Restored {
    pub session: SessionId,
    pub checkpoint: CheckpointId,
    pub path: String,
    /// None means the newly created file was moved to Trash.
    pub revision: Option<String>,
}
