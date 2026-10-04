use super::*;
use adk_core::{LlmResponse, LlmResponseStream, UsageMetadata};
use std::sync::Mutex;

struct Model {
    responses: Vec<LlmResponse>,
    requests: Mutex<Vec<LlmRequest>>,
    error: bool,
    pending: bool,
}

#[async_trait]
impl Llm for Model {
    fn name(&self) -> &str {
        "summary-fixture"
    }
    async fn generate_content(
        &self,
        request: LlmRequest,
        stream: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        assert!(stream);
        self.requests.lock().unwrap().push(request);
        if self.pending {
            return Ok(Box::pin(futures::stream::pending()));
        }
        let mut responses: Vec<_> = self.responses.iter().cloned().map(Ok).collect();
        if self.error {
            responses.push(Err(adk_core::AdkError::model(
                "private provider diagnostic",
            )));
        }
        Ok(Box::pin(futures::stream::iter(responses)))
    }
}

fn complete(text: &str) -> LlmResponse {
    LlmResponse {
        content: Some(Content::new("model").with_text(text)),
        turn_complete: true,
        finish_reason: Some(FinishReason::Stop),
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 100,
            candidates_token_count: 5,
            ..Default::default()
        }),
        provider_metadata: Some(json!({"sailry_timing":{"elapsed_us":250000}})),
        ..Default::default()
    }
}

fn summarizer(responses: Vec<LlmResponse>, error: bool) -> (Summarizer, Arc<Model>) {
    let model = Arc::new(Model {
        responses,
        requests: Mutex::default(),
        error,
        pending: false,
    });
    (
        Summarizer {
            model: model.clone(),
            turn: TurnId::new(),
            budget: Some(12800),
            output: Some(2048),
            stop: CancellationToken::new(),
        },
        model,
    )
}

mod budget {
    use super::*;

    #[test]
    fn reserves_output_capacity() {
        for (context, output, expected) in [
            (16_000, 2_048, 12_800),
            (128_000, 16_384, 102_400),
            (200_000, 64_000, 136_000),
            (1_050_000, 128_000, 840_000),
            (128_001, 16_384, 102_400),
            (u32::MAX, 128_000, 3_435_973_836),
        ] {
            let threshold = super::super::budget(context, output);
            assert_eq!(threshold, Some(expected));
            let (mut summarizer, _) = summarizer(vec![], false);
            summarizer.budget = threshold;
            assert!(!summarizer.needs_compaction(Some(expected - 1)));
            assert!(summarizer.needs_compaction(Some(expected)));
            assert!(summarizer.needs_compaction(Some(expected + 1)));
            assert!(!summarizer.needs_compaction(None));
        }
    }

    #[test]
    fn leaves_unknown_capacity_unbounded() {
        for (context, output) in [(0, 0), (0, 1024), (16_000, 16_000), (16_000, 32_000)] {
            assert_eq!(super::super::budget(context, output), None);
        }
        let (mut summarizer, _) = summarizer(vec![], false);
        summarizer.budget = None;
        assert!(!summarizer.needs_compaction(Some(u64::MAX)));
    }
}

fn history() -> Vec<Event> {
    let mut first = Event::new("completed");
    first.author = "user".into();
    first.set_content(Content::new("user").with_text("Keep requirements 中文 🙂 ".repeat(40)));
    let mut last = Event::new("completed");
    last.author = "assistant".into();
    last.set_content(Content::new("model").with_text("Verified output"));
    vec![first, last]
}

#[tokio::test]
async fn persists_valid_summaries() {
    let (summarizer, model) = summarizer(
        vec![
            LlmResponse {
                content: Some(Content::new("model").with_text("partial")),
                partial: true,
                ..Default::default()
            },
            complete("Complete summary 中文 🙂"),
        ],
        false,
    );
    let history = history();
    let summary = summarizer
        .summarize_events(&history)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        summary.provider_metadata[crate::store::agent::compaction::END_EVENT],
        history.last().unwrap().id
    );
    assert_eq!(summary.invocation_id, summarizer.turn.to_string());
    assert!(summary.content().is_none());
    assert_eq!(
        summary.llm_response.provider_metadata.as_ref().unwrap()["sailry_timing"]["elapsed_us"],
        250000
    );
    assert_eq!(
        summary
            .llm_response
            .usage_metadata
            .unwrap()
            .prompt_token_count,
        100
    );
    let content = summary.actions.compaction.unwrap().compacted_content;
    assert_eq!(content.role, "model");
    assert_eq!(
        content.parts,
        Content::new("model")
            .with_text("Complete summary 中文 🙂")
            .parts
    );
    let requests = model.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].tools.is_empty());
    assert!(requests[0].previous_response_id.is_none());
    assert!(
        requests[0].contents[1].parts[0]
            .text()
            .unwrap()
            .contains("Keep requirements 中文 🙂")
    );
}

#[tokio::test]
async fn rejects_invalid_summaries() {
    let history = history();
    let mut tool = complete("Summary");
    tool.content
        .as_mut()
        .unwrap()
        .parts
        .push(Part::FunctionCall {
            name: "write_file".into(),
            args: json!({}),
            id: None,
            thought_signature: None,
        });
    let mut truncated = complete("Summary");
    truncated.finish_reason = Some(FinishReason::MaxTokens);
    let mut failed = complete("Summary");
    failed.error_message = Some("private diagnostic".into());
    for (responses, error) in [
        (vec![complete("")], false),
        (vec![complete(&"x".repeat(16000))], false),
        (vec![complete(&"x".repeat(SUMMARY_BYTES + 1))], false),
        (
            vec![LlmResponse {
                content: Some(Content::new("model").with_text("partial")),
                partial: true,
                ..Default::default()
            }],
            false,
        ),
        (vec![complete("Summary")], true),
        (vec![tool], false),
        (vec![truncated], false),
        (vec![failed], false),
    ] {
        let (summarizer, model) = summarizer(responses, error);
        let result = summarizer.summarize_events(&history).await;
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(!error.to_string().contains("private"));
        assert!(!model::error(error).message.contains("private"));
        assert_eq!(model.requests.lock().unwrap().len(), 1);
    }
}

#[test]
fn retains_summary_failure_reason() {
    for message in [
        "context summary timed out",
        "context summary did not reduce the conversation",
        "context summary exceeds the size limit",
        "context summary was not completed",
        "context summary stream ended before completion",
    ] {
        assert_eq!(model::error(invalid(message)).message, message);
    }
}

#[tokio::test]
async fn skips_ineligible_work() {
    let (summarizer, model) = summarizer(vec![], false);
    assert!(summarizer.summarize_events(&[]).await.unwrap().is_none());
    let mut small = history();
    small[0].set_content(Content::new("user").with_text("Small"));
    assert!(summarizer.summarize_events(&small).await.unwrap().is_none());
    summarizer.stop.cancel();
    assert!(
        summarizer
            .summarize_events(&history())
            .await
            .unwrap()
            .is_none()
    );
    assert!(model.requests.lock().unwrap().is_empty());
}

#[test]
fn preserves_transcript_evidence() {
    let mut events = history();
    let content = events[1].llm_response.content.as_mut().unwrap();
    content.parts.extend([
        Part::FunctionCall {
            name: "read_file".into(),
            args: json!({"path":"资料.txt"}),
            id: Some("read-1".into()),
            thought_signature: None,
        },
        Part::FunctionResponse {
            id: Some("read-1".into()),
            function_response: adk_core::FunctionResponseData::new(
                "read_file",
                json!({"text":"Verified evidence 🙂"}),
            ),
            annotations: None,
        },
        Part::FileData {
            file_uri: "sailry-attachment://fixture".into(),
            mime_type: "image/png".into(),
            annotations: None,
        },
        Part::InlineData {
            mime_type: "audio/wav".into(),
            data: b"binary payload".to_vec(),
            uri: None,
            annotations: None,
        },
    ]);
    let transcript = transcript(&events).unwrap();
    for expected in [
        "read_file",
        "read-1",
        "资料.txt",
        "Verified evidence 🙂",
        "sailry-attachment://fixture",
        "audio/wav",
        "contents_omitted",
    ] {
        assert!(transcript.contains(expected), "{expected}");
    }
    assert!(!transcript.contains("binary payload"));
}

#[tokio::test]
async fn discards_cancelled_summary() {
    let model = Arc::new(Model {
        responses: vec![],
        requests: Mutex::default(),
        error: false,
        pending: true,
    });
    let stop = CancellationToken::new();
    let summarizer = Summarizer {
        model: model.clone(),
        turn: TurnId::new(),
        budget: None,
        output: None,
        stop: stop.clone(),
    };
    let running = tokio::spawn(async move { summarizer.summarize_events(&history()).await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while model.requests.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    stop.cancel();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), running)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .is_none()
    );
}
