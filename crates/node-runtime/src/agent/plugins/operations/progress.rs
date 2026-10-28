//! The ADK tool result is the only durable progress record.
use crate::agent::catalog::CallContext;
use sailry_link::CancellationToken;
use sailry_protocol::conversation::progress::Progress;
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) struct UpdatePlan {
    pub name: String,
    pub description: Option<String>,
    pub session: String,
    pub stop: CancellationToken,
}

impl UpdatePlan {
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
                "title": {"type": ["string", "null"]},
                "steps": {"type": "array", "minItems": 1, "items": {
                    "type": "object", "additionalProperties": false, "required": ["description", "state"], "properties": {
                        "description": {"type": "string", "minLength": 1},
                        "state": {"type": "string", "enum": ["pending", "in_progress", "completed", "skipped"]}
                    }
                }}
            }}),
        )
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn execute(&self, context: Arc<CallContext>, arguments: Value) -> Result<Value, Fault> {
        if context.session_id() != self.session {
            return Err(Fault::invalid("task progress belongs to another session"));
        }
        if context.is_cancelled() || self.stop.is_cancelled() {
            return Err(Fault::invalid("task progress was cancelled"));
        }
        let progress: Progress = serde_json::from_value(arguments)
            .map_err(|_| Fault::invalid("invalid task progress arguments"))?;
        progress
            .validate()
            .map_err(|error| Fault::invalid(error.message))?;
        Ok(json!({"progress": progress}))
    }
}

crate::agent::catalog::provider!(UpdatePlan);
