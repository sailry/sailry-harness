//! Sailry's durable conversation format, independent of the execution engine.
mod content;
mod metadata;
mod validation;

pub(crate) use content::*;
pub(crate) use metadata::*;
pub(crate) use validation::validate;

use chrono::{DateTime, Utc};
use sailry_protocol::{TurnId, conversation::question};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    pub id: String,
    pub created: DateTime<Utc>,
    pub turn: TurnId,
    #[serde(default)]
    pub provenance: Provenance,
    #[serde(default)]
    pub metadata: Metadata,
    #[serde(flatten)]
    pub kind: Kind,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Provenance {
    /// The original speaker is also used by the public history projection.
    pub author: String,
    /// Tool receipts and captured failures match calls within this execution scope.
    pub branch: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Kind {
    Message(Message),
    Compaction(Compaction),
    Configuration(Configuration),
    Notice(Notice),
    Operation(Operation),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Message {
    pub role: Role,
    /// Includes omitted engine-only slots needed by stable public part references.
    #[serde(default)]
    pub part_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<Finish>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
    #[serde(default)]
    pub sources: Sources,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Role {
    User,
    #[default]
    Assistant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Finish {
    Completed,
    Interrupted,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ResponseError {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_input: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_output: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byok: Option<bool>,
    /// Provider-owned usage is retained without inventing normalized counters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub builtins: Vec<BuiltinUsage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct BuiltinUsage {
    pub name: String,
    pub count: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Sources {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub citations: Vec<Citation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grounding: Vec<Grounding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_suggestions: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Citation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Grounding {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Compaction {
    /// Canonical membership, rather than timestamps, identifies the replaced prefix.
    pub boundary: String,
    pub summary: Message,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<CompactedFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct CompactedFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Configuration {
    pub context: Context,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "notice", rename_all = "snake_case")]
pub(crate) enum Notice {
    Elicitation {
        parent: ToolIdentity,
        request: Value,
        spec: question::Spec,
        schema: Value,
    },
    ElicitationResult {
        call: String,
        response: question::Response,
    },
    PlanReview {
        source: String,
    },
    PlanResult {
        call: String,
        response: question::Response,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ToolIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub(crate) enum Operation {
    Visibility {
        message: String,
        user_visible: bool,
        agent_visible: bool,
    },
}

impl Record {
    pub(crate) fn new(turn: TurnId, kind: Kind) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            created: Utc::now(),
            turn,
            provenance: Provenance::default(),
            metadata: Metadata::default(),
            kind,
        }
    }

    pub(crate) fn context(&self) -> Option<&Context> {
        match &self.kind {
            Kind::Configuration(configuration) => Some(&configuration.context),
            _ => self.metadata.context.as_ref(),
        }
    }

    pub(crate) fn message(&self) -> Option<&Message> {
        match &self.kind {
            Kind::Message(message) => Some(message),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn message_mut(&mut self) -> Option<&mut Message> {
        match &mut self.kind {
            Kind::Message(message) => Some(message),
            _ => None,
        }
    }

    pub(crate) fn usage(&self) -> Option<&Usage> {
        match &self.kind {
            Kind::Message(message) => message.usage.as_ref(),
            Kind::Compaction(compaction) => compaction.summary.usage.as_ref(),
            _ => None,
        }
    }
}
