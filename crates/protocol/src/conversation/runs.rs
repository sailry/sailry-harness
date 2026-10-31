//! Bounded lifecycle metadata without message bodies or admission captures.
use super::Status;
use crate::{ErrorCode, SessionId, TurnId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub turn: TurnId,
    pub sequence: u64,
    pub status: Status,
    pub origin: Option<SessionId>,
    pub error_code: Option<ErrorCode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub session: SessionId,
    /// Structural history revision; status changes do not invalidate membership.
    pub revision: u64,
    pub runs: Vec<Summary>,
    pub next_before: Option<TurnId>,
    pub queue_revision: u64,
}
