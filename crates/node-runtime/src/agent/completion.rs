//! Tool-free plugin requests use GDK and the Node's frozen authorization.
use super::*;
use goose_provider_types::{
    base::collect_stream,
    conversation::message::{Message, MessageContentBlock},
};
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
    let binding = model::binding(
        ingress,
        provider,
        config,
        sailry_protocol::SessionId::new(),
        true,
        stop,
    )
    .await?;
    let messages = [Message::user().with_text(prompt)];
    let stream = tokio::select! { biased;
        _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "plugin model request was cancelled")),
        stream = responsive(binding.provider.stream(&binding.config, "", &messages, &[])) => stream?.map_err(model::error)?,
    };
    // GDK owns delta assembly. Only a stalled stream expires; reasoning remains progress.
    let responses = futures::stream::unfold(stream, |mut stream| async move {
        match responsive(stream.next()).await {
            Ok(Some(response)) => Some((response, stream)),
            Ok(None) => None,
            Err(error) => Some((
                Err(goose_provider_types::errors::ProviderError::NetworkError(
                    error.message,
                )),
                stream,
            )),
        }
    });
    let (message, usage) = tokio::select! { biased;
        _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "plugin model request was cancelled")),
        response = collect_stream(Box::pin(responses)) => response.map_err(model::error)?,
    };
    if message.content.iter().any(|part| {
        !matches!(
            part,
            MessageContentBlock::Text(_)
                | MessageContentBlock::Thinking(_)
                | MessageContentBlock::RedactedThinking(_)
        )
    }) {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "plugin model returned non-text output",
        ));
    }
    let text = message.as_concat_text();
    if text.len() > 64 * 1024 {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "plugin model response exceeds the size limit",
        ));
    }
    if text.trim().is_empty() {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "plugin model returned no text",
        ));
    }
    Ok(Text {
        text,
        model: config.model.clone(),
        tokens: usage
            .usage
            .total_tokens
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or(0),
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
