use crate::conversation::Run;
use serde::{Deserialize, Serialize};

pub const MAX_CATALOG_BYTES: usize = 8 * 1024 * 1024;

/// Host-wide resources for authorized activity observers, without settings or history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub node: crate::NodeId,
    pub cursor: u64,
    pub projects: Vec<crate::Project>,
    pub worktrees: Vec<crate::Worktree>,
    pub sessions: Vec<crate::Session>,
    pub terminals: Vec<crate::terminal::Info>,
}

/// Session activity and durable attention published by the execution Node.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub title: String,
    pub attention: Attention,
    pub run: Option<Run>,
    pub waiting: Option<Waiting>,
    pub queued: u32,
}

/// Optional board content is loaded separately from the shared Node snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    pub session: crate::SessionId,
    pub turn: Option<crate::TurnId>,
    pub text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Waiting {
    Approval,
    Input,
}

/// Node-owned unread state shared by every controller. Its revision is independent
/// of session configuration and guards acknowledgements against newer completions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attention {
    pub revision: u64,
    pub unread: bool,
}
