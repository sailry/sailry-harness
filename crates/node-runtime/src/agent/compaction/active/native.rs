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
        "Summarize completed context while retaining original history, the current tool batch, and two recent tool exchanges. Use only when context needs reducing. Returns compacted, skipped or failed; below half the compaction trigger, it skips without changing context. This does not complete the task or authorize further work."
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
        match self.compact(context.clone()).await {
            Err(error)
                if !self.summarizer.stop.is_cancelled()
                    && !context.is_cancelled()
                    && (failure::transient(&error) || failure::projectable(&error)) =>
            {
                self.notice(&error, SummaryMode::Manual, true).await?;
                Ok(json!({"status":"failed", "error":model::error(error).message}))
            }
            Err(error) => {
                if !self.summarizer.stop.is_cancelled() && !context.is_cancelled() {
                    self.halt(&error, SummaryMode::Manual).await?;
                }
                Err(error)
            }
            result => result,
        }
    }
}

impl Context {
    async fn compact(&self, context: Arc<dyn ToolContext>) -> adk_core::Result<Value> {
        let _boundary = self.gate.lock().await;
        self.discard_candidate()?;
        let history = self.history().await?;
        let events = &history.events;
        let mut request =
            LlmRequest::new(self.summarizer.model.name(), vec![self.preamble().clone()]);
        request
            .contents
            .extend(super::super::history::prefix(events));
        request.tools = prompt::session::declarations(events)?;
        let tokens = self.tokens(&request, &[]).await?;
        let skipped = json!({
            "status":"skipped", "context_tokens":tokens,
            "trigger_tokens":self.summarizer.budget(),
        });
        if self
            .summarizer
            .budget()
            .is_some_and(|budget| tokens.saturating_mul(2) < budget)
        {
            return Ok(skipped);
        }
        let Some(cut) = self.summarizer.prefix(
            events,
            self.candidates(&history, 2).into_iter(),
            self.summarizer.recent(),
        )?
        else {
            return Ok(skipped);
        };
        let prior = &events[..cut];
        if prior.len() == 1
            && prior[0]
                .provider_metadata
                .contains_key(crate::store::agent::compaction::END_EVENT)
        {
            return Ok(skipped);
        }
        let Some(summary) = self.summarize(prior, false, &request.tools).await? else {
            if self.summarizer.stop.is_cancelled() {
                return Err(AdkError::tool("context compaction was cancelled"));
            }
            return Ok(skipped);
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
        // Later automatic callbacks must retain the exchanges protected here.
        self.retention.store(2, Ordering::Relaxed);
        self.applied.store(true, Ordering::Relaxed);
        let current = self.history().await?;
        matching::rebuild(&mut request, &current.events, self.preamble(), "")?;
        let revisions = model::history::revisions(&current.events);
        let tokens = self.retain(&mut request, &revisions)?;
        // The summary is durable even if fixed retained context cannot fit.
        // The next ordinary model boundary still enforces its input capacity.
        self.invalidate(&request, &[]).await?;
        Ok(
            json!({"status":"compacted", "summary_id":id, "context_tokens":tokens, "trigger_tokens":self.summarizer.budget()}),
        )
    }
}
