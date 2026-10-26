//! Bounded conversation summaries and explicitly requested canonical tool details.
use super::{Content, Diagnostic, Failure, LocalText, Table, projection::Format};
use crate::{
    SessionId, TurnId, WorktreeId,
    conversation::{Image, progress::Progress, question::Spec},
    plugin::desktop::Icon,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_DETAIL_PAGE_BYTES: usize = 256 * 1024;
pub const DETAIL_PAGE_BYTES: usize = 64 * 1024;

/// Identity includes the visible part and its immutable public projection revision.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Reference {
    pub session: SessionId,
    pub turn: TurnId,
    pub entry: String,
    pub index: usize,
    pub call: Option<String>,
    pub revision: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<Resolved>,
    pub has_details: bool,
    /// Existing answer publications are loaded by the visible publication surface.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub published: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    pub diagnostics: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<super::Expected>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub raw_result: bool,
    pub images: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_id: Option<String>,
    #[serde(default, skip_serializing_if = "Delegation::is_empty")]
    pub delegation: Delegation,
    /// Interactive requests keep their validated input specification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<Box<Spec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<Progress>,
}

/// Frozen selection facts preserve child ownership checks without raw arguments.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delegation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeId>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invalid: bool,
}

impl Delegation {
    fn is_empty(&self) -> bool {
        self.session.is_none() && self.worktree.is_none() && !self.invalid
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    pub label: Option<LocalText>,
    pub input: Input,
    pub output: Option<Output>,
    pub approval: Vec<LocalText>,
    pub status: Option<LocalText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<crate::process::Outcome>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input {
    pub summary: Option<Text>,
    pub context: Option<Text>,
    pub target: Option<Text>,
    pub content: Option<Text>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    pub text: String,
    pub code: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    pub preview: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preview_deferred: bool,
    pub notices: Vec<LocalText>,
    pub paths: Vec<String>,
    pub body: Option<Body>,
    pub table: Option<Table>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Body {
    pub text: String,
    pub path: Option<String>,
    pub diff: Option<Format>,
    pub empty: Option<LocalText>,
}

/// Complete public values are kept outside conversation snapshots and row models.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    pub arguments: Value,
    pub result: Option<Value>,
    pub resolved: Option<Resolved>,
    pub content: Option<Content>,
    pub images: Vec<Image>,
}

/// UTF-8 JSON pages can split large strings without requiring one oversized frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub reference: Reference,
    pub offset: usize,
    pub total_bytes: usize,
    pub next_offset: Option<usize>,
    pub text: String,
}
