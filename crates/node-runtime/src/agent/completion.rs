//! Tool-free plugin requests use the Node's frozen model authorization.
use super::*;
use futures::StreamExt;
use model::{Content, Event, Message, Role};
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
    let messages = vec![Message {
        role: Role::User,
        content: vec![Content::Text { text: prompt }],
        ..Message::default()
    }];
    let request = binding.config.request(String::new(), messages, Vec::new());
    let mut stream = tokio::select! { biased;
        _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "plugin model request was cancelled")),
        stream = responsive(binding.provider.call(request, stop.clone())) => stream?.map_err(model::error)?,
    };
    let mut text = String::new();
    let mut text_block = false;
    let mut tokens = 0;
    let mut non_text = false;
    loop {
        let response = tokio::select! { biased;
            _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "plugin model request was cancelled")),
            response = responsive(stream.next()) => response?,
        };
        let Some(response) = response else {
            break;
        };
        match response.map_err(model::error)? {
            Event::TextDelta { text: delta, .. } => {
                if !text_block && !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&delta);
                text_block = true;
            }
            Event::Usage(usage) => tokens = usage.total.unwrap_or(0),
            Event::ToolCall { .. } | Event::ToolCallDelta { .. } => non_text = true,
            Event::ReasoningDelta { .. } | Event::RedactedThinking { .. } => text_block = false,
            Event::Finish(_) => {}
        }
    }
    if non_text {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "plugin model returned non-text output",
        ));
    }
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
