//! Temporary ADK runner boundary; provider behavior is exclusively GDK.
//! Stage 4 removes this module when the state machine consumes GDK messages directly.
use super::*;
use adk_core::{
    AdkError, Content, LlmRequest, LlmResponse, LlmResponseStream, Part, SchemaAdapter,
};
use async_trait::async_trait;
use futures::StreamExt;
use goose_provider_types::{
    base::{MessageStream, collect_stream},
    conversation::{
        message::{Message, MessageContentBlock},
        token_usage::ProviderUsage,
    },
    errors::ProviderError,
};
use serde_json::json;
use tokio::sync::mpsc;

mod request;
mod response;

pub(super) struct Model {
    pub binding: Binding,
    pub stop: CancellationToken,
}

#[async_trait]
impl Llm for Model {
    fn name(&self) -> &str {
        &self.binding.config.model_name
    }
    fn schema_adapter(&self) -> &dyn SchemaAdapter {
        static GENERIC: adk_core::GenericSchemaAdapter = adk_core::GenericSchemaAdapter;
        &GENERIC
    }

    async fn generate_content(
        &self,
        request: LlmRequest,
        streaming: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        let (config, system, messages, tools) = request::convert(request, &self.binding.config)?;
        let responses = tokio::select! {
            biased;
            _ = self.stop.cancelled() => return Err(cancelled()),
            result = self.binding.provider.stream(&config, &system, &messages, &tools) => result.map_err(failure)?,
        };
        let (sender, receiver) = mpsc::channel(1);
        let stop = self.stop.child_token();
        let interrupted = stop.clone();
        let model = config.model_name;
        let provider = self.binding.provider.get_name().to_owned();
        tokio::spawn(async move {
            let partials = sender.clone();
            let chunks: MessageStream = Box::pin(responses.then(move |item| {
                let sender = partials.clone();
                async move {
                    if streaming && let Ok((Some(message), _)) = &item {
                        let parts: Vec<_> = message
                            .content
                            .iter()
                            .filter_map(|block| match block {
                                MessageContentBlock::Text(text) => Some(Part::Text {
                                    text: text.text.clone(),
                                }),
                                MessageContentBlock::Thinking(thinking) => Some(Part::Thinking {
                                    thinking: thinking.thinking.clone(),
                                    signature: (!thinking.signature.is_empty())
                                        .then(|| thinking.signature.clone()),
                                }),
                                _ => None,
                            })
                            .collect();
                        if !parts.is_empty() {
                            let _ = sender
                                .send(Ok(LlmResponse {
                                    content: Some(Content {
                                        role: "model".into(),
                                        parts,
                                    }),
                                    partial: true,
                                    ..Default::default()
                                }))
                                .await;
                        }
                    }
                    item
                }
            }));
            let result = tokio::select! {
                biased;
                _ = interrupted.cancelled() => Err(cancelled()),
                result = collect_stream(chunks) => result.map_err(failure).and_then(|(message,usage)| response::convert(message, usage, &model, &provider)),
            };
            let _ = sender.send(result).await;
        });
        Ok(Box::pin(Output { receiver, stop }))
    }
}

struct Output {
    receiver: mpsc::Receiver<adk_core::Result<LlmResponse>>,
    stop: CancellationToken,
}
impl futures::Stream for Output {
    type Item = adk_core::Result<LlmResponse>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.receiver.poll_recv(context)
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(super) fn failure(error: ProviderError) -> AdkError {
    use adk_core::{ErrorCategory, ErrorComponent};
    let (category, code) = match &error {
        ProviderError::Authentication(_) => {
            (ErrorCategory::Unauthorized, "model.gdk.authentication")
        }
        ProviderError::ContextLengthExceeded(_) => (
            ErrorCategory::ResourceExhausted,
            "model.sailry.context_overflow",
        ),
        ProviderError::RateLimitExceeded { .. } | ProviderError::CreditsExhausted { .. } => {
            (ErrorCategory::RateLimited, "model.gdk.quota")
        }
        ProviderError::InvalidValue(_) => (ErrorCategory::InvalidInput, "model.gdk.invalid"),
        ProviderError::NotConfigured => (ErrorCategory::InvalidInput, "model.gdk.not_configured"),
        _ => (ErrorCategory::Unavailable, "model.gdk.request"),
    };
    let fault = errors::fault(error.clone());
    let mut result =
        AdkError::new(ErrorComponent::Model, category, code, error.to_string()).with_source(fault);
    if let ProviderError::RateLimitExceeded {
        retry_delay: Some(delay),
        ..
    } = error
    {
        let retry = result.retry.clone().with_retry_after(delay);
        result = result.with_retry(retry);
    }
    result
}

fn cancelled() -> AdkError {
    AdkError::new(
        adk_core::ErrorComponent::Model,
        adk_core::ErrorCategory::Cancelled,
        "model.gdk.cancelled",
        "model request was cancelled",
    )
}

#[cfg(test)]
mod tests;
