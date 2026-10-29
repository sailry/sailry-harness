use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// An original part slot remains stable when engine-only blocks are removed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Block {
    pub index: usize,
    #[serde(flatten)]
    pub kind: BlockKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum BlockKind {
    Text {
        text: String,
    },
    Image {
        asset: AssetRef,
    },
    Document {
        asset: AssetRef,
    },
    File {
        file: FileRef,
    },
    ToolRequest(ToolRequest),
    ToolResponse(ToolResponse),
    Thinking {
        text: String,
        signature: Option<String>,
    },
    RedactedThinking {
        data: Vec<String>,
        /// Original ordinary part to precede; `None` places a block last.
        #[serde(default)]
        before: Vec<Option<usize>>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ToolRequest {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    /// GDK's provider-owned ToolRequest.metadata must round-trip unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_meta: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ToolError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ToolResponse {
    pub id: String,
    pub name: String,
    pub status: ToolStatus,
    /// Original JSON is stored once; known binary slots are externalized below.
    pub result: Payload,
    /// Existing output references hash the original hydrated JSON serialization.
    pub revision: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "ToolFormat::is_json")]
    pub format: ToolFormat,
    /// MCP result metadata is separate from provider request metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Map<String, Value>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolFormat {
    #[default]
    Json,
    Mcp,
    McpError,
}

impl ToolFormat {
    fn is_json(&self) -> bool {
        *self == Self::Json
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolStatus {
    Success,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Attachment {
    Inline { asset: AssetRef },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AssetRef {
    pub digest: String,
    pub mime: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileRef {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Payload {
    pub value: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<BinarySlot>,
}

impl From<Value> for Payload {
    fn from(value: Value) -> Self {
        Self {
            value,
            assets: Vec::new(),
        }
    }
}

/// A JSON pointer preserves the position and encoding of a removed binary value.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct BinarySlot {
    pub pointer: String,
    pub asset: AssetRef,
    pub encoding: BinaryEncoding,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BinaryEncoding {
    Bytes,
    Base64,
}
