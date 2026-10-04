//! Explicit requests share the automatic summary and model-context projection.
use super::*;
use adk_core::{AdkError, Tool, ToolContext};
use serde_json::Value;

#[async_trait]
impl Tool for Context {
    fn name(&self) -> &str {
        "compact_context"
    }

    fn description(&self) -> &str {
        "Summarize completed context while retaining original history, the current tool batch, and two recent tool exchanges. Use only when context needs reducing. Returns compacted or skipped; a skipped request leaves context unchanged. This does not complete the task or authorize further work."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(json!({"type":"object", "properties":{}, "required":[], "additionalProperties":false}))
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        if context.session_id() != self.session.to_string() {
            return Err(AdkError::tool(
                "context compaction belongs to another session",
            ));
        }
        if context.is_cancelled() || self.summarizer.stop.is_cancelled() {
            return Err(AdkError::tool("context compaction was cancelled"));
        }
        if !arguments.as_object().is_some_and(serde_json::Map::is_empty) {
            return Err(AdkError::tool("context compaction accepts an empty object"));
        }
        let events = self.events().await?;
        let start = events
            .iter()
            .position(|event| event.invocation_id == context.invocation_id())
            .ok_or_else(|| AdkError::tool("context compaction has no active invocation"))?;
        // The current batch has not yielded its results yet. Keep it and the
        // latest two completed exchanges outside the summary boundary.
        let cut = events
            .iter()
            .enumerate()
            .filter(|(_, event)| {
                event.invocation_id == context.invocation_id()
                    && event.content().is_some_and(|content| {
                        content
                            .parts
                            .iter()
                            .any(|part| matches!(part, Part::FunctionCall { .. }))
                    })
            })
            .rev()
            .nth(2)
            .map(|(index, _)| index)
            .filter(|cut| crate::store::agent::compaction::complete_exchange(&events[start..*cut]))
            .unwrap_or(start);
        let prior = &events[..cut];
        if prior.len() == 1
            && prior[0]
                .provider_metadata
                .contains_key(crate::store::agent::compaction::END_EVENT)
        {
            return Ok(json!({"status":"skipped"}));
        }
        let Some(summary) = self.summarizer.summarize_events(prior).await? else {
            if self.summarizer.stop.is_cancelled() {
                return Err(AdkError::tool("context compaction was cancelled"));
            }
            return Ok(json!({"status":"skipped"}));
        };
        if context.is_cancelled() || self.summarizer.stop.is_cancelled() {
            return Err(AdkError::tool("context compaction was cancelled"));
        }
        let id = summary.id.clone();
        self.service
            .append_event(&self.session.to_string(), summary)
            .await?;
        self.applied.store(true, Ordering::Relaxed);
        Ok(json!({"status":"compacted", "summary_id":id}))
    }
}
