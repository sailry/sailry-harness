//! Execution-owned attachment bytes travel over authenticated streams, not events.
use crate::{AttachmentId, StreamId, WorktreeId};
use serde::{Deserialize, Serialize};

pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec {
    pub worktree: WorktreeId,
    pub name: String,
    pub media_type: String,
    pub size: u64,
    /// Full-content BLAKE3 hash, verified before publication.
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: AttachmentId,
    pub spec: Spec,
}

/// Single-use staging, consumed by a durable FinishAttachmentUpload command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upload {
    pub stream: StreamId,
    pub spec: Spec,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    pub stream: StreamId,
    pub attachment: Attachment,
}
