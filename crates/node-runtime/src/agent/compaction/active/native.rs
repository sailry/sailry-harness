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
        let result = self.compact(context.clone()).await;
        if let Err(error) = &result
            && !self.summarizer.stop.is_cancelled()
            && !context.is_cancelled()
        {
            self.notice(error, SummaryMode::Manual, true).await?;
        }
        result
    }
}

impl Context {
    async fn compact(&self, context: Arc<dyn ToolContext>) -> adk_core::Result<Value> {
        let history = self.history().await?;
        let events = &history.events;
        let Some(cut) = self
            .summarizer
            .prefix(events, self.candidates(&history, 2).into_iter())?
        else {
            return Ok(json!({"status":"skipped"}));
        };
        let prior = &events[..cut];
        if prior.len() == 1
            && prior[0]
                .provider_metadata
                .contains_key(crate::store::agent::compaction::END_EVENT)
        {
            return Ok(json!({"status":"skipped"}));
        }
        let Some(summary) = self
            .summarize(prior, false, history.frontier.as_deref())
            .await?
        else {
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
            .await
            .map_err(save_error)?;
        *self
            .checkpoint
            .lock()
            .map_err(|_| invalid("context checkpoint is unavailable"))? = None;
        *self
            .attempted
            .lock()
            .map_err(|_| invalid("context attempt state is unavailable"))? = None;
        // Later automatic callbacks must retain the exchanges protected here.
        self.retention.store(2, Ordering::Relaxed);
        self.manual_applied.store(true, Ordering::Relaxed);
        self.applied.store(true, Ordering::Relaxed);
        Ok(json!({"status":"compacted", "summary_id":id}))
    }
}
