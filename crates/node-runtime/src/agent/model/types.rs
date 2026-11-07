use async_trait::async_trait;
use futures::stream::BoxStream;
use sailry_protocol::Effort;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
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
    pub headers: reqwest::header::HeaderMap,
    pub context_limit: Option<usize>,
    pub reasoning: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Message {
    pub id: Option<String>,
    pub created: i64,
    pub role: Role,
    pub content: Vec<Content>,
    pub metadata: Metadata,
}

impl Default for Message {
    fn default() -> Self {
        Self {
            id: None,
            created: chrono::Utc::now().timestamp(),
            role: Role::Assistant,
            content: Vec::new(),
            metadata: Metadata::default(),
        }
    }
}

impl Message {
    pub(crate) fn user() -> Self {
        Self {
            role: Role::User,
            ..Self::default()
        }
    }
    pub(crate) fn assistant() -> Self {
        Self::default()
    }
    pub(crate) fn with_content(mut self, content: Content) -> Self {
        self.content.push(content);
        self
    }
    pub(crate) fn with_text(self, text: impl Into<String>) -> Self {
        self.with_content(Content::Text { text: text.into() })
    }
    pub(crate) fn with_thinking(
        self,
        text: impl Into<String>,
        signature: impl Into<String>,
    ) -> Self {
        self.with_content(Content::Thinking {
            text: text.into(),
            signature: Some(signature.into()),
        })
    }
    pub(crate) fn with_image(
        self,
        data_base64: impl Into<String>,
        media_type: impl Into<String>,
    ) -> Self {
        self.with_content(Content::Image {
            media_type: media_type.into(),
            data: Image::Inline {
                data_base64: data_base64.into(),
            },
            annotations: None,
        })
    }
    pub(crate) fn with_document(
        self,
        data_base64: impl Into<String>,
        media_type: impl Into<String>,
        name: Option<String>,
    ) -> Self {
        self.with_content(Content::Document {
            media_type: media_type.into(),
            name,
            data_base64: data_base64.into(),
        })
    }
    pub(crate) fn with_tool_request(
        self,
        id: impl Into<String>,
        call: Result<rmcp::model::CallToolRequestParams, rmcp::model::ErrorData>,
    ) -> Self {
        let (name, arguments, error) = match call {
            Ok(call) => (
                call.name.into_owned(),
                Value::Object(call.arguments.unwrap_or_default()),
                None,
            ),
            Err(error) => (
                String::new(),
                Value::Null,
                Some(ToolError {
                    code: error.code.0,
                    message: error.message.into_owned(),
                    data: error.data,
                }),
            ),
        };
        self.with_content(Content::ToolCall(ToolCall {
            id: id.into(),
            name,
            arguments,
            error,
            metadata: None,
            tool_meta: None,
        }))
    }
    pub(crate) fn with_tool_response(
        self,
        id: impl Into<String>,
        result: Result<rmcp::model::CallToolResult, rmcp::model::ErrorData>,
    ) -> Self {
        self.with_content(Content::ToolResult(ToolResult {
            id: id.into(),
            name: String::new(),
            result,
            metadata: None,
        }))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Metadata {
    pub user_visible: bool,
    pub agent_visible: bool,
    pub inference: Option<Inference>,
    pub output_token_limit_reached: bool,
    pub steer: bool,
    pub turn_context: bool,
    pub compaction_summary: bool,
    pub usage: Option<Box<Usage>>,
    pub operation_logs: Vec<String>,
    pub operations: Option<Box<BTreeMap<String, Map<String, Value>>>>,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            user_visible: true,
            agent_visible: true,
            inference: None,
            output_token_limit_reached: false,
            steer: false,
            turn_context: false,
            compaction_summary: false,
            usage: None,
            operation_logs: Vec::new(),
            operations: None,
        }
    }
}

impl Metadata {
    pub(crate) fn operation_note(&self, operation: &str, key: &str) -> Option<&Value> {
        self.operations.as_ref()?.get(operation)?.get(key)
    }

    pub(crate) fn set_operation_note(&mut self, operation: &str, key: &str, value: Value) {
        self.operations
            .get_or_insert_with(Box::default)
            .entry(operation.to_owned())
            .or_default()
            .insert(key.to_owned(), value);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Inference {
    pub provider: String,
    pub requested_model: String,
    pub resolved_model: Option<String>,
    pub provider_session_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Config {
    pub model_name: String,
    pub context_limit: Option<usize>,
    pub max_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub thinking_effort: Effort,
    pub reasoning: Option<bool>,
    pub supports_vision: Option<bool>,
    pub request_params: Map<String, Value>,
}

impl Config {
    pub(crate) fn request(
        &self,
        system: String,
        messages: Vec<Message>,
        tools: Vec<Tool>,
    ) -> Request {
        Request {
            model: self.model_name.clone(),
            system,
            messages,
            tools,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            effort: self.thinking_effort,
            params: self.request_params.clone(),
            context_limit: self.context_limit,
            reasoning: self.reasoning,
            ..Request::default()
        }
    }
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
    Error {
        code: Option<String>,
        message: String,
    },
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

impl From<rmcp::model::Tool> for Tool {
    fn from(tool: rmcp::model::Tool) -> Self {
        Self {
            name: tool.name.into_owned(),
            description: tool.description.map(|description| description.into_owned()),
            parameters: Value::Object(tool.input_schema.as_ref().clone()),
        }
    }
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
    pub code: i32,
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
    pub elapsed_ms: Option<u64>,
    pub time_to_first_token_ms: Option<u64>,
    pub is_compaction: bool,
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
