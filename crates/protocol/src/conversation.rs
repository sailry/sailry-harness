//! UI-independent conversation projections; execution-engine types stay on the Node.
pub mod assets;
pub mod catalog;
pub mod cloud;
pub mod discovery;
pub mod login;
pub mod oauth;
pub mod progress;
pub mod question;
pub mod reasoning;
pub mod reference;
pub mod search;
use crate::{CredentialRef, Fault, NodeId, ProviderId, SessionId, TurnId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Sent and queued input; attachment bytes belong to the execution Node.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input {
    pub text: String,
    pub attachments: Vec<crate::AttachmentId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<reference::Reference>,
}

/// Atomically creates a conversation and admits its first ordinary Agent turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Start {
    pub project: Option<crate::ProjectId>,
    pub worktree: Option<crate::WorktreeId>,
    /// Omitted configuration resolves the execution Node's defaults at admission.
    pub config: Option<crate::SessionConfig>,
    pub title: String,
    pub message: Input,
}

impl From<String> for Input {
    fn from(text: String) -> Self {
        Self {
            text,
            attachments: Vec::new(),
            references: Vec::new(),
        }
    }
}

impl From<&str> for Input {
    fn from(text: &str) -> Self {
        text.to_owned().into()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelApi {
    ChatCompletions,
    Responses,
    Anthropic,
    Gemini,
    #[serde(rename = "deepseek")]
    DeepSeek,
    OpenCodeGo,
    OpenCodeZen,
    AzureOpenAi,
    AzureAi,
    Bedrock,
    Vertex,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provider {
    pub id: ProviderId,
    pub revision: u64,
    pub name: String,
    pub api: ModelApi,
    pub authentication: crate::Authentication,
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<cloud::Options>,
    /// Non-secret request metadata; omission uses the shared vendor defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<oauth::Options>,
    pub enabled: bool,
    #[serde(default)]
    pub models: Vec<Model>,
    pub default_model: String,
    pub credential: Option<CredentialRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub context: u32,
    pub output: u32,
    #[serde(default)]
    pub vision: bool,
    #[serde(default)]
    pub tools: bool,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub web_search: bool,
    #[serde(default)]
    pub generates: Vec<crate::media::Generation>,
    #[serde(default)]
    pub efforts: Vec<crate::Effort>,
    /// Explicit choices and defaults are not replaced by catalog completion.
    #[serde(default)]
    pub custom_efforts: bool,
    #[serde(default)]
    pub default_effort: crate::Effort,
}

#[cfg(test)]
#[path = "conversation/model_tests.rs"]
mod model_tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Running,
    Stopping,
    Completed,
    Cancelled,
    Interrupted,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    Task,
    Compaction,
}

#[cfg(test)]
#[path = "conversation/run_tests.rs"]
mod run_tests;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    /// Execution location frozen when the turn is admitted.
    pub worktree: crate::WorktreeId,
    pub turn: TurnId,
    pub kind: RunKind,
    pub session: SessionId,
    /// Stable admission order on the execution Node, including reordered queued turns.
    pub sequence: u64,
    pub revision: u64,
    pub status: Status,
    pub error: Option<Fault>,
    /// Execution-node timestamps, excluding time spent queued.
    pub started_ms: Option<i64>,
    /// Unknown if execution ended without an observed terminal transition.
    pub finished_ms: Option<i64>,
    /// Original configuration/history owner for inherited, closed turns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<SessionId>,
}

/// A child execution referenced by its parent's canonical tool call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Child {
    pub run: Run,
    pub origin: crate::Delegation,
    /// Display name from the parent's frozen role, absent for a generic task.
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fork {
    pub session: SessionId,
    pub through: TurnId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rewind {
    pub session: SessionId,
    pub revision: u64,
    pub through: Option<TurnId>,
    pub backup: crate::Session,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queue {
    /// Changes when contents, order, readiness or pause state change.
    pub revision: u64,
    pub paused: bool,
    pub items: Vec<Pending>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub turn: TurnId,
    pub kind: RunKind,
    /// Editable message revision, independent of the frozen session revision.
    pub revision: u64,
    pub config_revision: u64,
    pub ready: bool,
    pub preview: String,
    pub truncated: bool,
    pub attachments: Vec<crate::attachment::Attachment>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedMessage {
    pub turn: crate::QueuedTurn,
    pub revision: u64,
    pub message: Input,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueUpdate {
    pub session: SessionId,
    pub queue: Queue,
    pub runs: Vec<Run>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approve,
    Deny,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Pending,
    Approved,
    Denied,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalSource {
    /// A granted mutation of the captured plugin's own private values.
    Storage,
    User,
    Project,
    Full,
}

/// Authorization metadata references the exact call in canonical ADK history.
/// Approved does not imply that the tool executed or completed successfully.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub id: crate::ApprovalId,
    pub session: SessionId,
    pub turn: TurnId,
    pub entry: String,
    pub index: usize,
    pub state: ApprovalState,
    pub source: ApprovalSource,
}

/// Image bytes remain in canonical message history on the execution Node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub entry: String,
    /// Original content part and inline image positions, before projection.
    pub part: usize,
    pub index: usize,
    pub attachment: crate::attachment::Attachment,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Part {
    Text(String),
    Reference(reference::Reference),
    Attachment(crate::attachment::Attachment),
    Image(Image),
    Thinking(String),
    /// A durable context summary; original messages remain in history.
    Compaction(String),
    ToolCall {
        id: Option<String>,
        name: String,
        arguments: Value,
        #[serde(default)]
        presentation: crate::tool::Presentation,
        #[serde(default)]
        grouping: crate::tool::Grouping,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display: Option<Box<crate::tool::Display>>,
    },
    ToolResult {
        id: Option<String>,
        name: String,
        result: Value,
        images: Vec<Image>,
    },
    /// Retains non-text content for consumers that support that resource kind.
    Resource(Value),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// All prompt tokens, including cache reads and writes.
    pub input: u64,
    /// All generated tokens, including reasoning when reported by the provider.
    pub output: u64,
    /// A subset of input; do not add it to the total again.
    pub cached_input: u64,
    /// A subset of output; do not add it to the total again.
    pub reasoning: u64,
}

/// Statistics for all visible canonical history, independent of loaded pages.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statistics {
    /// Turns with at least one committed event; unsent queue items are excluded.
    pub turns: u64,
    /// Canonical responses that reported usage, not HTTP attempts or estimated billing.
    pub responses: u64,
    /// Absent when no response reported token usage.
    pub usage: Option<Usage>,
    /// Latest model response's input plus output, not cumulative session usage.
    /// An estimate of occupied context; unknown after compaction or an unmetered response.
    pub context_tokens: Option<u64>,
    pub cost: Option<crate::usage::Cost>,
    pub generation: Option<crate::usage::Generation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Citation {
    pub uri: String,
    pub title: Option<String>,
    /// Unicode character offsets across the entry's concatenated text parts.
    /// Absent when the provider does not identify a text range.
    pub start: Option<u32>,
    pub end: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub sequence: u64,
    pub id: String,
    pub turn: TurnId,
    pub author: String,
    pub branch: String,
    pub timestamp_ms: i64,
    pub parts: Vec<Part>,
    pub citations: Vec<Citation>,
    /// Provider-authored HTML that must accompany the unmodified grounded answer.
    pub search_suggestions: Option<String>,
    pub usage: Option<Usage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub session: SessionId,
    /// Structural history revision; appending events does not change it.
    pub revision: u64,
    pub entries: Vec<Entry>,
    pub runs: Vec<Run>,
    pub queue: Queue,
    pub approvals: Vec<Approval>,
    pub questions: Vec<question::Question>,
    pub children: Vec<Child>,
    /// Logical history pages start and end at turn boundaries.
    pub next_before: Option<TurnId>,
}

/// A bounded transport response. Client hydrates `missing` before publishing the page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    /// Stream watermark at the read; only comparable within one subscription generation.
    pub sequence: u64,
    pub page: Page,
    pub missing: Vec<TurnId>,
}

/// Internal continuation for a large turn, not another user-visible history page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnHistory {
    pub sequence: u64,
    pub revision: u64,
    pub run: Run,
    pub entries: Vec<Entry>,
    pub approvals: Vec<Approval>,
    pub questions: Vec<question::Question>,
    pub children: Vec<Child>,
    pub next_before: Option<u64>,
}

/// In-progress model output, not committed conversation history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub id: String,
    pub turn: TurnId,
    pub author: String,
    pub branch: String,
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub node: NodeId,
    /// Transient stream sequence, reset by a new subscription generation.
    pub sequence: u64,
    pub page: std::sync::Arc<Page>,
    pub missing: Vec<TurnId>,
    pub drafts: Vec<Draft>,
    pub statistics: Statistics,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Change {
    /// Published only after the canonical event transaction commits.
    Entry(Entry),
    Statistics(Statistics),
    Run(Run),
    Queue(Queue),
    Approval(Approval),
    Question(question::Question),
    Child(Child),
    /// Append text/thinking parts to the matching in-progress draft.
    Delta(Draft),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub node: NodeId,
    pub session: SessionId,
    pub sequence: u64,
    pub change: Change,
}
pub mod checkpoint;

#[cfg(test)]
#[path = "conversation/part_tests.rs"]
mod part_tests;

#[cfg(test)]
mod input {
    use super::*;

    #[test]
    fn retains_references() {
        let input = Input {
            references: Vec::new(),
            text: "input 中文 🙂".into(),
            attachments: vec![crate::AttachmentId::new()],
        };
        let wire = serde_json::to_value(&input).unwrap();
        assert_eq!(wire["attachments"][0], input.attachments[0].to_string());
        assert_eq!(serde_json::from_value::<Input>(wire).unwrap(), input);
        assert_eq!(
            serde_json::to_value(Input::from("text")).unwrap()["attachments"],
            serde_json::json!([])
        );
    }
}
