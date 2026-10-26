use serde::{Deserialize, Serialize};

/// Identifies one immutable JSON result in the conversation's visible history.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputReference {
    pub call: String,
    /// BLAKE3 of the original result's compact JSON, before model projection.
    pub revision: String,
}

/// A UTF-8 page of the original compact JSON, without copying its storage.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutputPage {
    pub turn: crate::TurnId,
    pub output: OutputReference,
    pub offset: usize,
    pub total_bytes: usize,
    pub next_offset: Option<usize>,
    pub text: String,
}

pub const MAX_OUTPUT_PAGE_BYTES: usize = 32 * 1024;
