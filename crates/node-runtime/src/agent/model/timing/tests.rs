use super::*;
use adk_core::{Content, LlmResponse, UsageMetadata};
use async_trait::async_trait;
use std::sync::Mutex;
use tracing::instrument::WithSubscriber as _;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn dispatch(&self) -> tracing::Dispatch {
        tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .json()
                .without_time()
                .with_writer(self.clone())
                .finish(),
        )
    }

    fn records(&self) -> Vec<Value> {
        let bytes = self.0.lock().unwrap().clone();
        String::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

impl std::io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for Capture {
    type Writer = Self;

    fn make_writer(&'writer self) -> Self::Writer {
        self.clone()
    }
}

struct Model;

struct Cancelled {
    stop: sailry_link::CancellationToken,
}

#[async_trait]
impl Llm for Cancelled {
    fn name(&self) -> &str {
        "cancelled-fixture"
    }

    async fn generate_content(
        &self,
        _: LlmRequest,
        _: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        self.stop.cancel();
        Err(adk_core::AdkError::new(
            adk_core::ErrorComponent::Model,
            adk_core::ErrorCategory::Cancelled,
            "model.request_cancelled",
            "PRIVATE_CANCEL_MESSAGE",
        ))
    }
}

#[async_trait]
impl Llm for Model {
    fn name(&self) -> &str {
        "timing-fixture"
    }
    async fn generate_content(
        &self,
        _: LlmRequest,
        _: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let response = LlmResponse {
            content: Some(Content::new("model").with_text("Timed output")),
            usage_metadata: Some(UsageMetadata {
                candidates_token_count: 4,
                ..Default::default()
            }),
            provider_metadata: Some(
                json!({"citation":"preserved","sailry_timing":{"elapsed_us":1}}),
            ),
            ..Default::default()
        };
        Ok(Box::pin(futures::stream::iter([
            Ok(LlmResponse {
                partial: true,
                ..response.clone()
            }),
            Ok(response.clone()),
            Ok(LlmResponse {
                error_code: Some("fixture_error".into()),
                ..response
            }),
        ])))
    }
}

#[tokio::test]
async fn preserves_usage_timing() {
    let model: Arc<dyn Llm> = Arc::new(Model);
    let responses = generate(
        &model,
        LlmRequest::new(model.name(), vec![]),
        true,
        "conversation",
        &crate::logging::Context::default(),
        sailry_link::CancellationToken::new(),
    )
    .await
    .unwrap()
    .collect::<Vec<_>>()
    .await;
    for (index, response) in responses.into_iter().enumerate() {
        let metadata = response.unwrap().provider_metadata.unwrap();
        assert_eq!(metadata["citation"], "preserved");
        if index == 1 {
            let elapsed = metadata["sailry_timing"]["elapsed_us"].as_u64().unwrap();
            let first = metadata["sailry_timing"]["first_token_us"]
                .as_u64()
                .unwrap();
            assert!(first >= 1000 && first <= elapsed);
        } else {
            assert!(metadata.get("sailry_timing").is_none());
        }
    }
}

#[tokio::test]
async fn omits_nonstreaming_first_token() {
    let model: Arc<dyn Llm> = Arc::new(Model);
    let responses = generate(
        &model,
        LlmRequest::new(model.name(), vec![]),
        false,
        "conversation",
        &crate::logging::Context::default(),
        sailry_link::CancellationToken::new(),
    )
    .await
    .unwrap()
    .collect::<Vec<_>>()
    .await;
    let metadata = responses[1]
        .as_ref()
        .unwrap()
        .provider_metadata
        .as_ref()
        .unwrap();
    assert!(metadata["sailry_timing"]["first_token_us"].is_null());
    assert!(metadata["sailry_timing"]["elapsed_us"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn records_observed_cancellation_cause() {
    let stop = sailry_link::CancellationToken::new();
    let model: Arc<dyn Llm> = Arc::new(Cancelled { stop: stop.clone() });
    let capture = Capture::default();
    let result = generate(
        &model,
        LlmRequest::new(model.name(), vec![]),
        true,
        "model_request",
        &crate::logging::Context::default(),
        stop,
    )
    .with_subscriber(capture.dispatch())
    .await;
    let Err(error) = result else {
        panic!("cancelled model request must fail")
    };
    assert_eq!(error.code, "model.request_cancelled");
    let records = capture.records();
    assert_eq!(records.len(), 2);
    let fields = &records[1]["fields"];
    assert_eq!(fields["phase"], "cancelled");
    assert_eq!(fields["code"], "cancelled");
    assert_eq!(fields["cause_code"], "model.request_cancelled");
    assert_eq!(fields["cause_component"], "model");
    assert_eq!(fields["cause_category"], "cancelled");
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_")
    );
}
