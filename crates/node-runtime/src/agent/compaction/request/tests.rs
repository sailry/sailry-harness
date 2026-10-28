use super::*;
use crate::agent::compaction::tests::{complete, history, summarizer};
use adk_core::{FunctionResponseData, LlmResponse, LlmResponseStream};
use std::collections::VecDeque;
use std::sync::Mutex;

struct Sequence {
    responses: Mutex<VecDeque<adk_core::Result<LlmResponse>>>,
    requests: Mutex<Vec<LlmRequest>>,
}

#[async_trait]
impl Llm for Sequence {
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
        let response = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("bounded summary request count");
        Ok(Box::pin(futures::stream::iter([response])))
    }
}

fn setup(responses: Vec<LlmResponse>) -> (Summarizer, Arc<Sequence>, Vec<Event>) {
    outcomes(responses.into_iter().map(Ok).collect())
}

fn outcomes(
    responses: Vec<adk_core::Result<LlmResponse>>,
) -> (Summarizer, Arc<Sequence>, Vec<Event>) {
    let model = Arc::new(Sequence {
        responses: Mutex::new(responses.into()),
        requests: Mutex::default(),
    });
    let (mut summary, _) = summarizer(vec![], false);
    summary.model = model.clone();
    summary.context = Some(128_000);
    let mut events = history();
    events[1]
        .llm_response
        .content
        .as_mut()
        .unwrap()
        .parts
        .push(Part::FunctionCall {
            name: "mcp_fixture".into(),
            args: json!({}),
            id: Some("evidence".into()),
            thought_signature: None,
        });
    let mut result = Event::new("completed");
    result.author = "user".into();
    result.set_content(Content {
        role: "function".into(),
        parts: vec![Part::FunctionResponse {
            id: Some("evidence".into()),
            function_response: FunctionResponseData::new(
                "mcp_fixture",
                json!({
                    "text":"Exact historical evidence ".repeat(600)
                }),
            ),
            annotations: None,
        }],
    });
    events.push(result);
    (summary, model, events)
}

fn draft(text: &str) -> LlmResponse {
    let mut response = complete("unused");
    response.content = Some(Content::new("model").with_text(text));
    response
}

#[tokio::test]
async fn accepts_plain_handoff_once() {
    let (summary, model, events) = setup(vec![draft("Verified source findings")]);
    let original = serde_json::to_vec(&events).unwrap();
    let event = summary
        .summarize_with(&events, SummaryMode::Automatic)
        .await
        .unwrap()
        .unwrap();
    let content = event.actions.compaction.unwrap().compacted_content;
    assert_eq!(content.role, "user");
    assert!(
        content.parts[0]
            .text()
            .unwrap()
            .starts_with("Verified source findings")
    );
    let requests = model.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(serde_json::to_vec(&events).unwrap(), original);
}

#[tokio::test]
async fn accepts_plain_handoff_after_overflow() {
    let overflow = adk_core::AdkError::new(
        adk_core::ErrorComponent::Model,
        adk_core::ErrorCategory::InvalidInput,
        "model.fixture.invalid_request",
        "prompt is too long: 140000 tokens > 128000 maximum",
    )
    .with_upstream_status(400);
    let (summary, model, events) =
        outcomes(vec![Err(overflow), Ok(draft("Verified source findings"))]);
    let original = serde_json::to_vec(&events).unwrap();
    let event = summary
        .summarize_with(&events, SummaryMode::Automatic)
        .await
        .unwrap()
        .unwrap();
    assert!(
        event.actions.compaction.unwrap().compacted_content.parts[0]
            .text()
            .unwrap()
            .starts_with("Verified source findings")
    );
    let requests = model.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(estimate::request(&requests[1]).unwrap() < estimate::request(&requests[0]).unwrap());
    assert_eq!(serde_json::to_vec(&events).unwrap(), original);
}

#[tokio::test]
async fn leaves_reasoning_exhaustion_unsaved() {
    let mut exhausted = draft("");
    exhausted.finish_reason = Some(FinishReason::MaxTokens);
    let (summary, model, events) = setup(vec![exhausted.clone(), exhausted]);
    let error = summary
        .summarize_with(&events, SummaryMode::Automatic)
        .await
        .unwrap_err();
    assert_eq!(
        failure::classify(&error),
        (Stage::Summary, Reason::TruncatedSummary)
    );
    assert_eq!(model.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn keeps_focus_separate_from_user_history() {
    let (mut summary, model, events) = setup(vec![complete("Verified findings")]);
    summary.focus = Some("Keep the Git rebase and unresolved changes".into());
    summary
        .summarize_with(&events, SummaryMode::Automatic)
        .await
        .unwrap()
        .unwrap();
    let requests = model.requests.lock().unwrap();
    let request = &requests[0];
    let focus = request.contents.last().unwrap().parts[0].text().unwrap();
    assert!(focus.contains("User-requested compaction focus"));
    assert!(focus.contains("Keep the Git rebase and unresolved changes"));
    assert!(
        !serde_json::to_string(&events)
            .unwrap()
            .contains("Keep the Git rebase")
    );
}
