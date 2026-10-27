//! Retry one reduced request only before any model response has been published.
use super::*;
use adk_core::{LlmResponseStream, SchemaAdapter};
use futures::StreamExt;

struct Model {
    context: Arc<Context>,
    inner: Arc<dyn Llm>,
}

struct Responses {
    context: Arc<Context>,
    model: Arc<dyn Llm>,
    request: LlmRequest,
    stream: bool,
    responses: Option<LlmResponseStream>,
    published: bool,
    finished: bool,
}

pub(super) fn wrap(context: Arc<Context>, inner: Arc<dyn Llm>) -> Arc<dyn Llm> {
    Arc::new(Model { context, inner })
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
        request: LlmRequest,
        stream: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        let state = Responses {
            context: self.context.clone(),
            model: self.inner.clone(),
            request,
            stream,
            responses: None,
            published: false,
            finished: false,
        };
        Ok(Box::pin(futures::stream::unfold(
            state,
            |mut state| async move {
                if state.finished {
                    return None;
                }
                loop {
                    let next = match state.responses.as_mut() {
                        Some(responses) => responses.next().await,
                        None => match state
                            .model
                            .generate_content(state.request.clone(), state.stream)
                            .await
                        {
                            Ok(responses) => {
                                state.responses = Some(responses);
                                continue;
                            }
                            Err(error) => Some(Err(error)),
                        },
                    };
                    let recoverable = !state.published
                        && match &next {
                            Some(Ok(response)) => exhausted(response),
                            Some(Err(error)) => model::context_overflow(error) || length(error),
                            None => false,
                        };
                    if recoverable {
                        match state.recover().await {
                            Ok(true) => continue,
                            Ok(false) => {}
                            Err(error) => {
                                let error = state.halt(error).await;
                                return Some((Err(error), state));
                            }
                        }
                    }
                    match next {
                        Some(Ok(response)) => {
                            state.published |= response
                                .content
                                .as_ref()
                                .is_some_and(|content| content.parts.iter().any(visible));
                            return Some((Ok(response), state));
                        }
                        Some(Err(error)) if model::context_overflow(&error) => {
                            let error = state.halt(request_error(error)).await;
                            return Some((Err(error), state));
                        }
                        Some(Err(error)) => {
                            state.finished = true;
                            return Some((Err(error), state));
                        }
                        None => return None,
                    }
                }
            },
        )))
    }
}

impl Responses {
    /// Output exhausted before any content forces one compaction of a smaller
    /// request; otherwise the original outcome is published unchanged.
    async fn recover(&mut self) -> adk_core::Result<bool> {
        if self.context.summarizer.stop.is_cancelled() || !self.context.state.overflow() {
            return Ok(false);
        }
        let before = estimate::request(&self.request)?;
        let request = self
            .context
            .transform("", self.request.clone(), &[], true)
            .await?;
        if estimate::request(&request)? >= before {
            return Ok(false);
        }
        self.responses.take();
        self.request = request;
        Ok(true)
    }

    async fn halt(&mut self, error: adk_core::AdkError) -> adk_core::AdkError {
        self.finished = true;
        self.responses.take();
        self.context
            .halt(&error, SummaryMode::Automatic)
            .await
            .err()
            .unwrap_or(error)
    }
}

fn exhausted(response: &adk_core::LlmResponse) -> bool {
    response.finish_reason == Some(adk_core::FinishReason::MaxTokens)
        && !response.partial
        && response.content.as_ref().is_none_or(|content| {
            content.parts.iter().all(|part| match part {
                Part::Text { text } => text.trim().is_empty(),
                Part::Thinking { .. } | Part::ProviderData { .. } => true,
                _ => false,
            })
        })
}

fn visible(part: &Part) -> bool {
    match part {
        Part::Text { text } => !text.is_empty(),
        Part::Thinking {
            thinking,
            signature,
        } => !thinking.is_empty() || signature.is_some(),
        _ => true,
    }
}

fn length(error: &adk_core::AdkError) -> bool {
    error
        .details
        .metadata
        .get("incomplete_reason")
        .and_then(serde_json::Value::as_str)
        == Some("max_output_tokens")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(parts: Vec<Part>) -> adk_core::LlmResponse {
        adk_core::LlmResponse {
            content: Some(Content {
                role: "model".into(),
                parts,
            }),
            finish_reason: Some(adk_core::FinishReason::MaxTokens),
            ..Default::default()
        }
    }

    #[test]
    fn detects_output_exhaustion_before_content() {
        assert!(exhausted(&response(Vec::new())));
        assert!(exhausted(&response(vec![Part::Text { text: " ".into() }])));
        assert!(!exhausted(&response(vec![Part::Text {
            text: "partial answer".into()
        }])));
        let mut stopped = response(Vec::new());
        stopped.finish_reason = Some(adk_core::FinishReason::Stop);
        assert!(!exhausted(&stopped));
        let mut streaming = response(Vec::new());
        streaming.partial = true;
        assert!(!exhausted(&streaming));
    }

    #[test]
    fn detects_incomplete_length() {
        let mut error = adk_core::AdkError::model("incomplete response");
        assert!(!length(&error));
        error
            .details
            .metadata
            .insert("incomplete_reason".into(), json!("max_output_tokens"));
        assert!(length(&error));
        error
            .details
            .metadata
            .insert("incomplete_reason".into(), json!("content_filter"));
        assert!(!length(&error));
    }

    #[test]
    fn ignores_empty_stream_envelopes() {
        assert!(!visible(&Part::Text {
            text: String::new()
        }));
        assert!(!visible(&Part::Thinking {
            thinking: String::new(),
            signature: None
        }));
        assert!(visible(&Part::Text {
            text: "Published answer".into()
        }));
        assert!(visible(&Part::Thinking {
            thinking: "Published reasoning".into(),
            signature: None
        }));
    }
}
