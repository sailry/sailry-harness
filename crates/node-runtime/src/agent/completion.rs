//! Tool-free plugin requests reuse ADK model adapters, credentials and account auth.
use super::*;
use adk_core::{Content, FinishReason, LlmRequest, Part};
use sailry_protocol::{SessionConfig, conversation::Provider, plugin::Text};

async fn responsive<T>(future: impl std::future::Future<Output = T>) -> Result<T, Fault> {
    tokio::time::timeout(std::time::Duration::from_secs(120), future)
        .await
        .map_err(|_| Fault::new(ErrorCode::Unavailable, "plugin model request timed out"))
}

pub(crate) async fn generate(
    ingress: &Arc<Ingress>,
    provider: &Provider,
    config: &SessionConfig,
    prompt: String,
    stop: &CancellationToken,
) -> Result<Text, Fault> {
    let model = model::configured(
        ingress,
        provider,
        config,
        sailry_protocol::SessionId::new(),
        true,
        stop,
    )
    .await?;
    let request = LlmRequest {
        model: config.model.clone(),
        contents: vec![Content::new("user").with_text(prompt)],
        ..LlmRequest::new(config.model.clone(), vec![])
    };
    let mut stream = responsive(model.generate_content(request, true))
        .await?
        .map_err(model::error)?;
    let mut text = String::new();
    let mut tokens = 0;
    let mut complete = false;
    // Active reasoning is progress, not a timeout. Only a stalled stream expires.
    while let Some(response) = responsive(stream.next()).await? {
        let response = response.map_err(model::error)?;
        if response.interrupted || response.error_code.is_some() || response.error_message.is_some()
        {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "plugin model response failed",
            ));
        }
        if let Some(usage) = response.usage_metadata {
            tokens = usage.total_token_count.max(0) as u64;
        }
        if let Some(content) = response.content {
            let mut value = String::new();
            for part in content.parts {
                match part {
                    Part::Text { text } => value.push_str(&text),
                    Part::Thinking { .. } => {}
                    _ => {
                        return Err(Fault::new(
                            ErrorCode::Unavailable,
                            "plugin model returned non-text output",
                        ));
                    }
                }
            }
            if response.partial {
                text.push_str(&value);
            } else if !value.is_empty() {
                text = value;
            }
            if text.len() > 64 * 1024 {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "plugin model response exceeds the size limit",
                ));
            }
        }
        if response.turn_complete {
            if response.finish_reason != Some(FinishReason::Stop) {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "plugin model response was not completed",
                ));
            }
            complete = true;
            // The terminal event already carries usage. A provider may keep its SSE
            // connection open after completion; socket EOF is not a turn boundary.
            break;
        }
    }
    if !complete || text.trim().is_empty() {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "plugin model returned no text",
        ));
    }
    Ok(Text {
        text,
        model: config.model.clone(),
        tokens,
    })
}

#[cfg(test)]
mod progress {
    use super::responsive;
    use std::time::Duration;

    #[tokio::test(start_paused = true)]
    async fn active_stream_outlives_idle_limit() {
        let started = tokio::time::Instant::now();
        for _ in 0..3 {
            responsive(tokio::time::sleep(Duration::from_secs(90)))
                .await
                .unwrap();
        }
        assert!(started.elapsed() >= Duration::from_secs(270));
    }

    #[tokio::test(start_paused = true)]
    async fn stalled_stream_expires() {
        let fault = responsive(std::future::pending::<()>()).await.unwrap_err();
        assert_eq!(fault.message, "plugin model request timed out");
    }
}
