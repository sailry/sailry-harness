//! Session naming commits through the same ADK event as its tool result.
use adk_core::{AdkError, Tool, ToolContext};
use async_trait::async_trait;
use sailry_link::CancellationToken;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) struct SetTitle {
    pub session: String,
    pub stop: CancellationToken,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    title: String,
}

#[async_trait]
impl Tool for SetTitle {
    fn name(&self) -> &str {
        "set_session_title"
    }

    fn description(&self) -> &str {
        "Set a short, recognizable title for this conversation in the user's language. Choose it once the task is clear. Preserve it across follow-up messages; rename again only if the user requests it. This only changes this session's display title."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(
            json!({"type": "object", "additionalProperties": false, "required": ["title"], "properties": {
                "title": {"type": "string", "minLength": 1}
            }}),
        )
    }

    // Naming remains available in planning mode.
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        if context.session_id() != self.session {
            return Err(AdkError::tool("title belongs to another session"));
        }
        if context.is_cancelled() || self.stop.is_cancelled() {
            return Err(AdkError::tool("title update was cancelled"));
        }
        let arguments: Arguments = serde_json::from_value(arguments)
            .map_err(|_| AdkError::tool("invalid title arguments"))?;
        let title = arguments.title.trim();
        if title.is_empty() || title.chars().any(char::is_control) {
            return Err(AdkError::tool("title must be nonempty single-line text"));
        }
        let mut actions = context.actions();
        actions
            .state_delta
            .insert(crate::store::agent::SESSION_TITLE.into(), json!(title));
        context.set_actions(actions);
        Ok(json!({"title": title}))
    }
}
