//! The ADK tool result is the only durable progress record.
use adk_core::{AdkError, Tool, ToolContext};
use async_trait::async_trait;
use sailry_link::CancellationToken;
use sailry_protocol::conversation::progress::{MAX_DESCRIPTION_BYTES, MAX_STEPS, Progress};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) struct UpdatePlan {
    pub name: String,
    pub description: Option<String>,
    pub session: String,
    pub stop: CancellationToken,
}

#[async_trait]
impl Tool for UpdatePlan {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        self.description
            .as_deref()
            .unwrap_or("Record the current structured progress snapshot")
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(
            json!({"type": "object", "additionalProperties": false, "required": ["title", "steps"], "properties": {
                "title": {"type": ["string", "null"], "maxLength": 512},
                "steps": {"type": "array", "minItems": 1, "maxItems": MAX_STEPS, "items": {
                    "type": "object", "additionalProperties": false, "required": ["description", "state"], "properties": {
                        "description": {"type": "string", "minLength": 1, "maxLength": MAX_DESCRIPTION_BYTES},
                        "state": {"type": "string", "enum": ["pending", "in_progress", "completed", "skipped"]}
                    }
                }}
            }}),
        )
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        if context.session_id() != self.session {
            return Err(AdkError::tool("task progress belongs to another session"));
        }
        if context.is_cancelled() || self.stop.is_cancelled() {
            return Err(AdkError::tool("task progress was cancelled"));
        }
        let progress: Progress = serde_json::from_value(arguments)
            .map_err(|_| AdkError::tool("invalid task progress arguments"))?;
        progress
            .validate()
            .map_err(|error| AdkError::tool(error.message))?;
        Ok(json!({"progress": progress}))
    }
}
