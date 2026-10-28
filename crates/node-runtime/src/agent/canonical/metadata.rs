use sailry_protocol::{
    TurnId,
    conversation::{compaction, observation, recovery, reference},
    media, tool,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Metadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Input>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<Presentation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<Execution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Context>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<StateChanges>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<Maintenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<Runtime>,
}

/// Extension state keeps its existing ownership scopes; Sailry context stays typed.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct StateChanges {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub session: BTreeMap<String, Value>,
    /// Keys omit the former `app:` prefix.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub application: BTreeMap<String, Value>,
    /// Keys omit the former `user:` prefix.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub user: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Input {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<TurnId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<InputSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_part: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_part: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice_part: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notice_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<reference::Reference>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InputSource {
    User,
    Parent,
    Plugin,
    Notice,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Presentation {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub groupings: BTreeMap<String, tool::Grouping>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub displays: BTreeMap<String, tool::Display>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, tool::Presentation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<ToolFailure>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ToolFailure {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub failure: tool::Failure,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Execution {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<Purpose>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<media::Kind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<Timing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<observation::Request>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phases: Option<observation::Phases>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<recovery::Retry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_started: Option<ToolStarted>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Purpose {
    Main,
    Compaction,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Timing {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_token_us: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ToolStarted {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Context {
    pub captured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personal: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub guides: BTreeMap<String, Guide>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded_tools: Option<BTreeSet<String>>,
    /// Historical masks remain understood until the GDK runner replaces this owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked_outputs: Option<BTreeSet<(String, String)>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Guide {
    pub path: String,
    pub revision: String,
    pub content: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Maintenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_instruction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<compaction::Failure>,
}

/// Only execution state used by the GDK pipeline is stored here.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Runtime {
    pub user_visible: bool,
    pub agent_visible: bool,
    pub output_limit_reached: bool,
    pub steer: bool,
    pub turn_context: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference: Option<Inference>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            user_visible: true,
            agent_visible: true,
            output_limit_reached: false,
            steer: false,
            turn_context: false,
            inference: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Inference {
    pub provider: String,
    pub requested_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session: Option<String>,
}
