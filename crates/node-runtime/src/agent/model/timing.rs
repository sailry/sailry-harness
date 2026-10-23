//! Measure each ADK model request independently of queueing, tools and user input.
use adk_core::{Llm, LlmRequest, LlmResponseStream};
use futures::StreamExt;
use serde_json::{Value, json};
use std::{sync::Arc, time::Instant};

pub(in crate::agent) async fn generate(
    model: &Arc<dyn Llm>,
    request: LlmRequest,
    stream: bool,
    operation: &'static str,
    identity: &crate::logging::Context,
    stop: sailry_link::CancellationToken,
) -> adk_core::Result<LlmResponseStream> {
    let identity = super::call::identity(identity.clone());
    let started = Instant::now();
    let mut entry =
        crate::logging::Entry::new(crate::logging::Area::Model, crate::logging::Phase::Begin);
    entry.context = identity.clone();
    entry.operation = Some(operation);
    entry.stage = Some("request");
    entry.request_mode = model.uses_interactions_api().then_some("interactions");
    entry.stream = Some(stream);
    entry.budget = request
        .config
        .as_ref()
        .and_then(|config| config.max_output_tokens)
        .map(i64::from);
    let mut span = crate::logging::Span::begin(entry, started).cancel_when(stop.clone());
    let responses =
        match super::call::scope(identity.clone(), model.generate_content(request, stream)).await {
            Ok(responses) => responses,
            Err(error) => {
                super::diagnostics::finish(&mut span, &error, stop.is_cancelled());
                return Err(error);
            }
        };
    let mut first_token_us = None;
    Ok(super::call::stream(
        identity,
        Box::pin(responses.map(move |result| {
            span.entry.stage = Some("response");
            match result {
                Err(error) => {
                    super::diagnostics::finish(&mut span, &error, stop.is_cancelled());
                    Err(error)
                }
                Ok(mut response) => {
                    if stream
                        && first_token_us.is_none()
                        && response.content.as_ref().is_some_and(|content| {
                            content
                                .parts
                                .iter()
                                .any(|part| part.text().is_some_and(|text| !text.is_empty()))
                        })
                    {
                        first_token_us = u64::try_from(started.elapsed().as_micros()).ok();
                        span.entry.first_token_us = first_token_us;
                    }
                    if let Some(Value::Object(metadata)) = &mut response.provider_metadata {
                        metadata.remove("sailry_timing");
                    }
                    if !response.partial
                        && !response.interrupted
                        && response.error_code.is_none()
                        && response.error_message.is_none()
                        && response.usage_metadata.is_some()
                        && let Ok(elapsed_us) = u64::try_from(started.elapsed().as_micros())
                        && elapsed_us > 0
                        && let Value::Object(metadata) =
                            response.provider_metadata.get_or_insert_with(|| json!({}))
                    {
                        metadata.insert(
                            "sailry_timing".into(),
                            json!({
                                "elapsed_us": elapsed_us,
                                "first_token_us": first_token_us,
                            }),
                        );
                    }
                    if response.interrupted {
                        span.finish(
                            if stop.is_cancelled() {
                                crate::logging::Phase::Cancelled
                            } else {
                                crate::logging::Phase::Interrupted
                            },
                            Some(if stop.is_cancelled() {
                                sailry_protocol::ErrorCode::Cancelled
                            } else {
                                sailry_protocol::ErrorCode::OutcomeUnknown
                            }),
                        );
                    } else if response.error_code.is_some() || response.error_message.is_some() {
                        span.finish(
                            crate::logging::Phase::Failed,
                            Some(sailry_protocol::ErrorCode::Unavailable),
                        );
                    } else if !response.partial {
                        span.finish(crate::logging::Phase::Completed, None);
                    }
                    Ok(response)
                }
            }
        })),
    ))
}

#[cfg(test)]
mod tests;
