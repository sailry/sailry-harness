//! Provider diagnostics become public only after captured-credential redaction.
use super::*;
use adk_core::{AdkError, LlmRequest, LlmResponseStream, SchemaAdapter};
use async_trait::async_trait;
use futures::StreamExt;
use sailry_protocol::Secret;

struct Model {
    inner: Arc<dyn Llm>,
    secrets: Vec<Secret>,
}

pub(super) fn wrap(inner: Arc<dyn Llm>, secrets: Vec<Secret>) -> Arc<dyn Llm> {
    Arc::new(Model { inner, secrets })
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

    fn max_cost_micro_usd(&self, request: &adk_core::LlmRequest) -> Option<u64> {
        self.inner.max_cost_micro_usd(request)
    }

    async fn generate_content(
        &self,
        request: LlmRequest,
        stream: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        let secrets = self.secrets.clone();
        let response = self
            .inner
            .generate_content(request, stream)
            .await
            .map_err(|error| redact(error, &secrets))?;
        Ok(protect(response, secrets))
    }
}

fn protect(response: LlmResponseStream, secrets: Vec<Secret>) -> LlmResponseStream {
    Box::pin(response.map(move |result| result.map_err(|error| redact(error, &secrets))))
}

fn redact(mut error: AdkError, secrets: &[Secret]) -> AdkError {
    // These existing application policy failures remain terminal.
    if error.code.starts_with("model.openai_responses.") {
        let policy = match error
            .details
            .metadata
            .get("provider_error_code")
            .and_then(serde_json::Value::as_str)
        {
            Some("subscription_sharing_usage_limit_exceeded") => Some((
                adk_core::ErrorCategory::RateLimited,
                "model.openai_responses.subscription_sharing_usage_limit_exceeded",
            )),
            Some("subscription_sharing_usage_unavailable") => Some((
                adk_core::ErrorCategory::Unavailable,
                "model.openai_responses.subscription_sharing_usage_unavailable",
            )),
            _ => None,
        };
        if let Some((category, code)) = policy {
            error.category = category;
            error.code = code;
            error.retry = adk_core::RetryHint::default();
        }
    }
    error.message = safe(&error.message, secrets, 8192);
    if let Some(request_id) = &mut error.details.request_id {
        *request_id = safe(request_id, secrets, 256);
    }
    error.details.metadata.retain(|_, value| {
        let value = value.to_string();
        !secrets
            .iter()
            .any(|secret| !secret.expose().is_empty() && value.contains(secret.expose()))
    });
    error
}

fn safe(value: &str, secrets: &[Secret], limit: usize) -> String {
    // Reject oversized fields rather than cutting through an echoed credential.
    if value.len() > 64 * 1024 {
        return "[upstream diagnostic exceeded the size limit]".into();
    }
    let mut values: Vec<_> = secrets
        .iter()
        .map(Secret::expose)
        .filter(|value| !value.is_empty())
        .collect();
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    let mut value = value.to_owned();
    for secret in values {
        value = value.replace(secret, "[redacted]");
    }
    if value.len() > limit {
        let mut end = limit;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
        value.push_str("... [truncated]");
    }
    value
}

#[cfg(test)]
mod tests;
