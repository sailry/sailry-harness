//! Provider diagnostics become public only after captured-credential redaction.
use super::*;
use adk_core::{AdkError, LlmRequest, LlmResponseStream, SchemaAdapter};
use adk_model::openai::ProviderErrorDetails;
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
    let source = std::error::Error::source(&error);
    if let Some(details) = source.and_then(|source| source.downcast_ref::<ProviderErrorDetails>()) {
        let mut fields = serde_json::Map::new();
        for (name, value, limit) in [
            ("message", details.message.as_deref(), 8192),
            ("code", details.code.as_deref(), 256),
            ("type", details.kind.as_deref(), 256),
            ("param", details.param.as_deref(), 512),
            ("request_id", details.request_id.as_deref(), 256),
        ] {
            if let Some(value) = value {
                fields.insert(name.into(), safe(value, secrets, limit).into());
            }
        }
        if let Some(status) = details.status {
            fields.insert("status".into(), status.into());
        }
        if !fields.is_empty() {
            let body = serde_json::to_string_pretty(&fields).expect("diagnostic strings serialize");
            let fault = Fault::new(
                ErrorCode::Unavailable,
                format!("{}\n\n{body}", summary(&error)),
            );
            error.details.metadata.retain(|_, value| {
                let value = value.to_string();
                !secrets
                    .iter()
                    .any(|secret| !secret.expose().is_empty() && value.contains(secret.expose()))
            });
            return error.with_source(fault);
        }
    }
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
