use super::*;
use adk_core::{Content, LlmResponse};

struct Stalled {
    before_stream: bool,
}

#[async_trait]
impl Llm for Stalled {
    fn name(&self) -> &str {
        "stalled-fixture"
    }

    async fn generate_content(
        &self,
        _: LlmRequest,
        _: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        if self.before_stream {
            futures::future::pending::<()>().await;
        }
        Ok(Box::pin(futures::stream::pending()))
    }
}

#[tokio::test(start_paused = true)]
async fn bounds_initial_response() {
    for before_stream in [true, false] {
        let started = tokio::time::Instant::now();
        let result = open(
            &Stalled { before_stream },
            LlmRequest::new("fixture", vec![]),
            true,
        )
        .await;
        let error = result.err().expect("stalled response must time out");
        assert_eq!(started.elapsed(), RESPONSE_TIMEOUT);
        assert_eq!(error.code, "model.response_timeout");
        assert!(!retryable(&error));
    }
}

struct Streaming;

struct Thinking;

#[async_trait]
impl Llm for Thinking {
    fn name(&self) -> &str {
        "thinking-fixture"
    }

    async fn generate_content(
        &self,
        _: LlmRequest,
        _: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        Ok(Box::pin(futures::stream::once(async {
            tokio::time::sleep(std::time::Duration::from_secs(90)).await;
            Ok(LlmResponse {
                content: Some(Content::new("model").with_text("Finished reasoning")),
                turn_complete: true,
                ..Default::default()
            })
        })))
    }
}

#[tokio::test(start_paused = true)]
async fn allows_reasoning_before_first_content() {
    let mut stream = open(&Thinking, LlmRequest::new("fixture", vec![]), true)
        .await
        .unwrap();
    assert!(stream.next().await.unwrap().unwrap().turn_complete);
    assert!(stream.next().await.is_none());
}

#[tokio::test(start_paused = true)]
async fn does_not_replay_a_local_deadline() {
    let mut attempts = 0;
    let result = execute_with_retry_observer(
        &RetryConfig::default().with_max_retries(5),
        retryable,
        None,
        &mut || {
            attempts += 1;
            open(
                &Stalled {
                    before_stream: false,
                },
                LlmRequest::new("fixture", vec![]),
                true,
            )
        },
        |_, _, _| async { panic!("a local deadline must not start another model request") },
    )
    .await;
    assert_eq!(result.err().unwrap().code, "model.response_timeout");
    assert_eq!(attempts, 1);
}

#[async_trait]
impl Llm for Streaming {
    fn name(&self) -> &str {
        "streaming-fixture"
    }

    async fn generate_content(
        &self,
        _: LlmRequest,
        _: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        let first = LlmResponse {
            content: Some(Content::new("model").with_text("First output")),
            partial: true,
            ..Default::default()
        };
        Ok(Box::pin(futures::stream::iter([Ok(first)]).chain(
            futures::stream::iter(0..4).then(|index| async move {
                tokio::time::sleep(RESPONSE_TIMEOUT / 2).await;
                Ok(LlmResponse {
                    partial: index != 3,
                    turn_complete: index == 3,
                    ..Default::default()
                })
            }),
        )))
    }
}

#[tokio::test(start_paused = true)]
async fn activity_resets_idle_deadline_without_empty_messages() {
    let mut stream = open(&Streaming, LlmRequest::new("fixture", vec![]), true)
        .await
        .unwrap();
    assert!(stream.next().await.unwrap().unwrap().partial);
    assert!(stream.next().await.unwrap().unwrap().turn_complete);
    assert!(stream.next().await.is_none());
}

#[tokio::test(start_paused = true)]
async fn stops_an_idle_stream_after_output() {
    let source = futures::stream::once(async {
        Ok(LlmResponse {
            content: Some(Content::new("model").with_text("Started")),
            partial: true,
            ..Default::default()
        })
    })
    .chain(futures::stream::pending());
    let mut stream = responsive(Box::pin(source));
    assert!(stream.next().await.unwrap().unwrap().partial);
    let started = tokio::time::Instant::now();
    assert_eq!(
        stream.next().await.unwrap().unwrap_err().code,
        "model.response_timeout"
    );
    assert_eq!(started.elapsed(), RESPONSE_TIMEOUT);
    assert!(stream.next().await.is_none());
}
