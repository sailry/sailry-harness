use super::*;
use adk_core::{Content, LlmResponse, UsageMetadata};
use async_trait::async_trait;

struct Model;

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
    let responses = generate(&model, LlmRequest::new(model.name(), vec![]), true)
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
    let responses = generate(&model, LlmRequest::new(model.name(), vec![]), false)
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
