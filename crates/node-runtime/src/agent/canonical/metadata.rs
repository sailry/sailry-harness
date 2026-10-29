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
}

impl Context {
    pub(crate) fn state(&self) -> std::collections::HashMap<String, Value> {
        let mut state = std::collections::HashMap::new();
        for (key, value) in [
            ("sailry.session_title", &self.title),
            ("sailry.context.policy", &self.policy),
            ("sailry.context.environment", &self.environment),
            ("sailry.context.personal", &self.personal),
        ] {
            if let Some(value) = value {
                state.insert(key.into(), serde_json::json!(value));
            }
        }
        state.extend(
            self.guides
                .iter()
                .map(|(key, value)| (key.clone(), serde_json::json!(value))),
        );
        if let Some(value) = &self.loaded_tools {
            state.insert(
                "sailry.context.loaded_tools".into(),
                serde_json::json!(value),
            );
        }
        state
    }

    pub(crate) fn from_state(state: &std::collections::HashMap<String, Value>) -> Self {
        let string = |key: &str| state.get(key).and_then(Value::as_str).map(str::to_owned);
        Self {
            title: string("sailry.session_title"),
            policy: string("sailry.context.policy"),
            environment: string("sailry.context.environment"),
            personal: string("sailry.context.personal"),
            loaded_tools: state
                .get("sailry.context.loaded_tools")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok()),
            guides: state
                .iter()
                .filter(|(key, _)| key.starts_with("sailry.context.guide:"))
                .filter_map(|(key, value)| {
                    Some((key.clone(), serde_json::from_value(value.clone()).ok()?))
                })
                .collect(),
            ..Self::default()
        }
    }
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
    /// The execution engine's original timestamp remains in seconds at its boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<i64>,
    /// Model-only maintenance messages retain their source position in the conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    pub user_visible: bool,
    pub agent_visible: bool,
    pub output_limit_reached: bool,
    pub steer: bool,
    pub turn_context: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference: Option<Inference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<OperationNotes>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logs: Vec<String>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            created: None,
            after: None,
            user_visible: true,
            agent_visible: true,
            output_limit_reached: false,
            steer: false,
            turn_context: false,
            inference: None,
            notes: None,
            logs: Vec::new(),
        }
    }
}

/// Durable decisions used when Goose reloads the conversation between operations.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct OperationNotes {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub empty_response: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_subagent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cancelled_subagents: Vec<String>,
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
