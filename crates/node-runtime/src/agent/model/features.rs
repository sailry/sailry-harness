//! Translate captured model choices at the existing ADK request boundary.
use adk_core::{AdkError, Llm, LlmRequest, LlmResponseStream, SchemaAdapter};
use async_trait::async_trait;
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

struct Model {
    inner: Arc<dyn Llm>,
    selected: BTreeMap<String, String>,
}

pub(super) fn wrap(inner: Arc<dyn Llm>, selected: BTreeMap<String, String>) -> Arc<dyn Llm> {
    Arc::new(Model { inner, selected })
}

#[async_trait]
impl Llm for Model {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn schema_adapter(&self) -> &dyn SchemaAdapter {
        self.inner.schema_adapter()
    }
    fn uses_interactions_api(&self) -> bool {
        self.inner.uses_interactions_api()
    }
    fn max_cost_micro_usd(&self, request: &LlmRequest) -> Option<u64> {
        self.inner.max_cost_micro_usd(request)
    }

    async fn generate_content(
        &self,
        mut request: LlmRequest,
        stream: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        configure(&mut request, &self.selected)?;
        self.inner.generate_content(request, stream).await
    }
}

fn configure(
    request: &mut LlmRequest,
    selected: &BTreeMap<String, String>,
) -> adk_core::Result<()> {
    if selected.get("speed").is_some_and(|value| value == "fast") {
        let config = request.config.get_or_insert_with(Default::default);
        let options = config
            .extensions
            .entry("openai")
            .or_insert_with(|| json!({}));
        let options = options
            .as_object_mut()
            .ok_or_else(|| AdkError::model("model feature request options are invalid"))?;
        options.insert("service_tier".into(), json!("priority"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
