//! Durable plugin notices owned by the execution Node.
use crate::{NotificationId, SessionId};
use serde::{Deserialize, Serialize};

pub const LIMIT: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub title: String,
    pub message: String,
    pub kind: Kind,
    /// None opens the publishing plugin when it has a page; a session must be
    /// in the publishing plugin's captured scope.
    pub session: Option<SessionId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    pub id: NotificationId,
    pub sequence: u64,
    pub package: String,
    pub content: Draft,
    pub timestamp_ms: i64,
    pub read: bool,
}
