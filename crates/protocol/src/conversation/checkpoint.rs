//! File recovery is independent of conversation history rewinds.
use crate::{
    ChangeKind, ChangeStatus, CheckpointId, ErrorCode, Output, RequestOutcome, SessionId, TurnId,
    WorktreeId,
};
use serde::{Deserialize, Serialize};

pub mod hunk;

/// What the original write's durable receipt reports for one checkpoint's prepared content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Receipt {
    /// The write completed with exactly this prepared content.
    Written,
    /// The write may have changed the file; inspect it before relying on the outcome.
    Uncertain,
    /// The write is admitted and has no business outcome yet.
    Pending,
    /// The write was not admitted, failed or did not reach this file.
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub revision: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

impl File {
    /// A change set reports each file separately; only its matching content entry applies.
    pub fn receipt(&self) -> Receipt {
        let result = match &self.outcome {
            RequestOutcome::NotAdmitted => return Receipt::Absent,
            RequestOutcome::Admitted => return Receipt::Pending,
            RequestOutcome::Unknown => return Receipt::Uncertain,
            RequestOutcome::Completed(result) => result.as_ref(),
        };
        match result {
            Ok(Output::FileWritten(written))
                if written.path == self.path
                    && written.revision == self.after.revision
                    && written.size == self.after.size =>
            {
                Receipt::Written
            }
            Ok(Output::FileChanges(changes)) => changes
                .files
                .iter()
                .find(|file| {
                    file.path == self.path
                        && matches!(file.kind, ChangeKind::Added | ChangeKind::Modified)
                })
                .map_or(Receipt::Absent, |file| match file.status {
                    ChangeStatus::Applied
                        if file.revision.as_deref() == Some(self.after.revision.as_str())
                            && file.size == Some(self.after.size) =>
                    {
                        Receipt::Written
                    }
                    ChangeStatus::Unknown => Receipt::Uncertain,
                    _ => Receipt::Absent,
                }),
            Err(error) if error.code == ErrorCode::OutcomeUnknown => Receipt::Uncertain,
            _ => Receipt::Absent,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
