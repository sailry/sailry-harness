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

#[tokio::test(start_paused = true)]
async fn honors_and_bounds_server_delays() {
    for structured in [false, true] {
        for seconds in [2, 86_400] {
            let policy = RetryConfig::default().with_max_retries(2);
            let hint = adk_model::retry::ServerRetryHint {
                retry_after: Some(std::time::Duration::from_secs(seconds)),
            };
            let expected = hint.retry_after.unwrap().min(policy.max_delay);
            let mut attempts = 0;
            let mut observed = Vec::new();
            let started = tokio::time::Instant::now();
            let result = execute_with_retry_observer(
                &policy,
                retryable,
                Some(&hint),
                &mut || {
                    attempts += 1;
                    let mut error = AdkError::model("HTTP 429 rate limit");
                    if structured {
                        error.retry.retry_after_ms = Some(seconds * 1_000);
                    }
                    std::future::ready(if attempts < 3 { Err(error) } else { Ok(()) })
                },
                |attempt, limit, delay| {
                    observed.push((attempt, limit, delay));
                    std::future::ready(Ok(()))
                },
            )
            .await;
            assert!(result.is_ok());
            assert_eq!(attempts, 3);
            let second = if structured {
                expected
            } else {
                std::time::Duration::from_millis(500)
            };
            assert_eq!(observed, vec![(1, 2, expected), (2, 2, second)]);
            assert_eq!(started.elapsed(), expected + second);
        }
    }
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
    let error = stream.next().await.unwrap().unwrap_err();
    assert_eq!(error.code, "model.response_timeout");
    assert!(!retryable(&error));
    assert_eq!(started.elapsed(), RESPONSE_TIMEOUT);
    assert!(stream.next().await.is_none());
}
