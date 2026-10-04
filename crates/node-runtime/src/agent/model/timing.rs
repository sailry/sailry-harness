//! Measure each ADK model request independently of queueing, tools and user input.
use adk_core::{Llm, LlmRequest, LlmResponseStream};
use futures::StreamExt;
use serde_json::{Value, json};
use std::{sync::Arc, time::Instant};

pub(in crate::agent) async fn generate(
    model: &Arc<dyn Llm>,
    request: LlmRequest,
    stream: bool,
) -> adk_core::Result<LlmResponseStream> {
    let started = Instant::now();
    let responses = model.generate_content(request, stream).await?;
    let mut first_token_us = None;
    Ok(Box::pin(responses.map(move |result| {
        result.map(|mut response| {
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
            response
        })
    })))
}

#[cfg(test)]
mod tests;
