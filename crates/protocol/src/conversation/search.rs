//! Literal search of committed message text; tool payloads and transient drafts are excluded.
use crate::{SessionId, TurnId};
use serde::{Deserialize, Serialize};

pub const MAX_QUERY_BYTES: usize = 512;
pub const MAX_SNIPPET_BYTES: usize = 4096;
pub const MAX_MATCHES: u16 = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    pub text: String,
    pub case_sensitive: bool,
    /// Exclusive canonical event sequence from a previous result, newest first.
    pub before: Option<u64>,
    pub limit: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Match {
    pub turn: TurnId,
    pub turn_sequence: u64,
    pub entry: String,
    pub sequence: u64,
    pub part: usize,
    pub author: String,
    pub snippet: String,
    /// UTF-8 byte range within the snippet, never a character or UTF-16 offset.
    pub highlight: std::ops::Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub session: SessionId,
    pub revision: u64,
    /// One match per canonical entry, with its first matching text part.
    pub matches: Vec<Match>,
    /// Node scans are bounded and can return an empty page with a continuation.
    pub next_before: Option<u64>,
}
