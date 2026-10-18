//! Neutral v1 JSON exchanged with a pure model translation module.
use super::Framing;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: String,
    pub model: String,
    pub messages: Vec<Message>,
    pub system: Option<String>,
    pub tools: Vec<Tool>,
    pub tool_choice: Choice,
    pub reasoning: Reasoning,
    pub max_output_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    #[serde(default)]
    pub stop: Vec<String>,
    pub stream: bool,
    pub session: String,
    pub endpoint: String,
    #[serde(default)]
    pub options: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    User {
        parts: Vec<Part>,
    },
    Assistant {
        parts: Vec<Part>,
    },
    Tool {
        call_id: String,
        name: String,
        parts: Vec<Part>,
        is_error: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Part {
    Text {
        text: String,
    },
    Image {
        media_type: String,
        data: Image,
    },
    File {
        media_type: String,
        name: Option<String>,
        data_base64: String,
    },
    Reasoning {
        text: Option<String>,
        replay: Option<String>,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
    Json {
        value: Value,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Image {
    Inline { data_base64: String },
    Url { url: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Choice {
    Auto,
    None,
    Required,
    Named { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reasoning {
    pub effort: crate::Effort,
    pub replay: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepared {
    pub method: super::super::model_auth::Method,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
    pub credential: Option<Injection>,
    pub framing: Option<Framing>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Injection {
    pub header: String,
    pub template: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decode {
    pub state: Value,
    pub events: Vec<Event>,
    pub done: bool,
    pub status: Option<u16>,
    pub headers: Option<BTreeMap<String, Vec<String>>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decoded {
    pub state: Value,
    pub deltas: Vec<Delta>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Delta {
    Text {
        text: String,
    },
    Reasoning {
        text: Option<String>,
        replay: Option<String>,
    },
    ToolCall {
        index: u64,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: Option<String>,
    },
    Usage {
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        cached_input_tokens: Option<u64>,
        reasoning_tokens: Option<u64>,
    },
    Finish {
        reason: Finish,
    },
    Error {
        class: Class,
        message: String,
        retry_after_ms: Option<u64>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Finish {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Authentication,
    Permission,
    RateLimited,
    Quota,
    ContextOverflow,
    InvalidRequest,
    Unavailable,
    Unsupported,
    Unknown,
}
