use async_trait::async_trait;
use futures::stream::BoxStream;
use sailry_protocol::Effort;
use serde_json::{Map, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub(crate) type ModelStream = BoxStream<'static, Result<Event, Error>>;

#[async_trait]
pub(crate) trait Model: Send + Sync {
    async fn call(&self, request: Request, cancel: CancellationToken)
    -> Result<ModelStream, Error>;
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Request {
    pub model: String,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<Tool>,
    pub max_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub effort: Effort,
    pub params: Map<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Message {
    pub role: Role,
    pub content: Vec<Content>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Role {
    User,
    #[default]
    Assistant,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Content {
    Text {
        text: String,
    },
    Thinking {
        text: String,
        signature: Option<String>,
    },
    RedactedThinking {
        data: String,
    },
    Image {
        media_type: String,
        data: Image,
        annotations: Option<Value>,
    },
    Document {
        media_type: String,
        name: Option<String>,
        data_base64: String,
    },
    ToolCall(ToolCall),
    ToolResult(ToolResult),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Image {
    Inline { data_base64: String },
    Url { url: String },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub metadata: Option<Map<String, Value>>,
    pub tool_meta: Option<Value>,
    pub error: Option<ToolError>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToolResult {
    pub id: String,
    pub name: String,
    pub result: Result<rmcp::model::CallToolResult, rmcp::model::ErrorData>,
    pub metadata: Option<Map<String, Value>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToolError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Usage {
    pub model: Option<String>,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub total: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
    pub reasoning: Option<u64>,
    pub cost: Option<f64>,
    pub provider: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    TextDelta {
        index: usize,
        text: String,
    },
    ReasoningDelta {
        index: usize,
        text: Option<String>,
        signature: Option<String>,
    },
    RedactedThinking {
        index: usize,
        data: String,
    },
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
    ToolCall {
        index: usize,
        call: ToolCall,
    },
    Usage(Usage),
    Finish(Finish),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Finish {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub(crate) struct Error {
    pub kind: ErrorKind,
    pub message: String,
    pub retry_after: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    NotConfigured,
    Authentication,
    ContextOverflow,
    RateLimited,
    Quota,
    Network,
    Unavailable,
    InvalidRequest,
    InvalidResponse,
    Unsupported,
    Cancelled,
    Other,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retry_after: None,
        }
    }

    pub(crate) fn retryable(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::RateLimited | ErrorKind::Network | ErrorKind::Unavailable
        )
    }
}
