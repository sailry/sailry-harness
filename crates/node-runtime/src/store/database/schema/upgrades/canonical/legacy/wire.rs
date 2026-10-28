//! The finite former Sailry event encoding, used only by the startup upgrade.
//! Field shapes follow adk-core at 675c02b (Apache-2.0); see third_party_licenses/adk-LICENSE.
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Deserialize)]
pub(super) struct Event {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub author: String,
    pub branch: String,
    #[serde(flatten)]
    pub response: Response,
    pub actions: Actions,
    #[serde(default)]
    pub long_running_tool_ids: Vec<String>,
    #[serde(default, rename = "event_metadata")]
    pub metadata: HashMap<String, String>,
}

impl Event {
    pub(super) fn content(&self) -> Option<&Content> {
        self.response.content.as_ref()
    }
}

#[derive(Deserialize)]
pub(super) struct Actions {
    pub state_delta: HashMap<String, Value>,
    pub artifact_delta: HashMap<String, i64>,
    pub compaction: Option<Summary>,
    #[serde(flatten)]
    pub other: HashMap<String, Value>,
}

#[derive(Deserialize)]
pub(super) struct Summary {
    pub start_timestamp: DateTime<Utc>,
    pub end_timestamp: DateTime<Utc>,
    pub compacted_content: Content,
}

#[derive(Deserialize)]
pub(super) struct Response {
    pub content: Option<Content>,
    pub usage_metadata: Option<Usage>,
    pub finish_reason: Option<String>,
    pub citation_metadata: Option<Citations>,
    pub turn_complete: bool,
    pub interrupted: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub provider_metadata: Option<Value>,
    pub model: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Usage {
    pub prompt_token_count: i32,
    pub candidates_token_count: i32,
    pub total_token_count: i32,
    pub cache_read_input_token_count: Option<i32>,
    pub cache_creation_input_token_count: Option<i32>,
    pub thinking_token_count: Option<i32>,
    pub audio_input_token_count: Option<i32>,
    pub audio_output_token_count: Option<i32>,
    pub cost: Option<f64>,
    pub is_byok: Option<bool>,
    pub provider_usage: Option<Value>,
    #[serde(default)]
    pub builtin_tool_uses: Vec<Builtin>,
}

#[derive(Deserialize)]
pub(super) struct Builtin {
    pub kind: String,
    pub count: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Citations {
    #[serde(default)]
    pub citation_sources: Vec<Citation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Citation {
    pub uri: Option<String>,
    pub title: Option<String>,
    pub start_index: Option<i32>,
    pub end_index: Option<i32>,
}

#[derive(Clone, Deserialize)]
pub(super) struct Content {
    pub role: String,
    pub parts: Vec<Part>,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum Part {
    ConfigurationUpdate {
        #[serde(rename = "configuration_update")]
        _configuration_update: Value,
    },
    ProviderData {
        provider_data: ProviderData,
    },
    Thinking {
        thinking: String,
        signature: Option<String>,
    },
    Text {
        text: String,
    },
    InlineData {
        mime_type: String,
        data: Vec<u8>,
        uri: Option<String>,
        annotations: Option<Value>,
    },
    FileData {
        mime_type: String,
        file_uri: String,
        annotations: Option<Value>,
    },
    FunctionCall {
        name: String,
        args: Value,
        id: Option<String>,
        thought_signature: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    FunctionResponse {
        function_response: FunctionResponse,
        id: Option<String>,
        annotations: Option<Value>,
    },
    ServerToolCall {
        server_tool_call: Value,
    },
    ServerToolResponse {
        server_tool_response: Value,
    },
    EmbeddedResource {
        resource: Value,
    },
}

#[derive(Clone, Deserialize)]
pub(super) struct ProviderData {
    pub origin: Option<Value>,
    pub parts: Box<[Part; 1]>,
}

#[derive(Clone, Deserialize)]
pub(super) struct FunctionResponse {
    pub name: String,
    pub response: Value,
    #[serde(default)]
    pub inline_data: Vec<InlineData>,
    #[serde(default)]
    pub file_data: Vec<FileData>,
}

#[derive(Clone, Deserialize)]
pub(super) struct InlineData {
    pub mime_type: String,
    pub data: Vec<u8>,
    pub uri: Option<String>,
    pub annotations: Option<Value>,
}

#[derive(Clone, Deserialize)]
pub(super) struct FileData {
    pub mime_type: String,
    pub file_uri: String,
    pub annotations: Option<Value>,
}

#[derive(Deserialize)]
pub(super) struct ToolIdentity {
    pub tool_name: String,
    pub args: Value,
    pub function_call_id: Option<String>,
}
