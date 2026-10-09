use crate::{GitIndex, SessionId, TurnId, WorktreeId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub id: String,
    pub old_start: u32,
    pub new_start: u32,
    pub text: String,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub session: SessionId,
    pub turn: TurnId,
    pub worktree: WorktreeId,
    pub path: String,
    pub hunks: Vec<Hunk>,
    pub partial: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Reject,
    Stage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub session: SessionId,
    pub turn: TurnId,
    pub worktree: WorktreeId,
    pub path: String,
    pub hunk: String,
    pub operation: Operation,
    pub expected_revision: String,
    pub expected_index: Option<String>,
    pub expected_head: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    pub session: SessionId,
    pub turn: TurnId,
    pub path: String,
    pub hunk: String,
    pub revision: String,
    pub index: Option<GitIndex>,
}
