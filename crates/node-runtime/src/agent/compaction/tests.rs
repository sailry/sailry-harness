use super::*;
use adk_core::{LlmResponse, LlmResponseStream, UsageMetadata};
use std::sync::Mutex;

mod timing;

const DIAGNOSTIC: &str = "OpenAI Responses API error: 429 Too Many Requests rate_limit_error: Summary request was rejected (code: rate_limit_exceeded)";

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
            let mut error = adk_core::AdkError::new(
                adk_core::ErrorComponent::Model,
                adk_core::ErrorCategory::RateLimited,
                "model.openai_responses.rate_limited",
                DIAGNOSTIC,
            )
            .with_upstream_status(429);
            error
                .details
                .metadata
                .insert("body".into(), json!("private provider diagnostic"));
            responses.push(Err(error));
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
            identity: crate::logging::Context::default(),
            turn: TurnId::new(),
            context: Some(16_000),
            budget: Some(12800),
            output: Some(2048),
            instruction: None,
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
    fn retains_known_capacity_without_reserve() {
        for output in [0, 16_000, 32_000] {
            assert_eq!(super::super::budget(16_000, output), Some(12_800));
        }
    }

    #[tokio::test]
    async fn rejects_oversized_summary_before_request() {
        let (summarizer, model) = summarizer(vec![complete("Unused summary")], false);
        let mut events = history();
        events[1].set_content(Content::new("model").with_text("x".repeat(80_000)));
        assert!(!summarizer.fits(&events).unwrap());
        assert!(summarizer.summarize_events(&events).await.is_err());
        assert!(model.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn leaves_unknown_capacity_unbounded() {
        for (context, output) in [(0, 0), (0, 1024)] {
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
    first
        .provider_metadata
        .insert(crate::store::agent::INPUT_SOURCE.into(), "user".into());
    first
        .provider_metadata
        .insert(crate::store::agent::INPUT_PART.into(), "0".into());

    first.set_content(Content::new("user").with_text("Keep requirements 中文 🙂"));
    let mut last = Event::new("completed");
    last.author = "assistant".into();
    last.set_content(Content::new("model").with_text(format!(
        "Verified output\n{}",
        "Inspected source evidence 中文 🙂 ".repeat(80)
    )));
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
            .with_text("Complete summary 中文 🙂\n\nLatest user instruction (verbatim data)\nKeep requirements 中文 🙂")
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
    for (responses, rejected) in [
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
        let (summarizer, model) = summarizer(responses, rejected);
        let result = summarizer.summarize_events(&history).await;
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(!error.to_string().contains("private"));
        let fault = model::error(error);
        assert!(!fault.message.contains("private"));
        if rejected {
            assert!(fault.message.contains(DIAGNOSTIC));
            assert!(
                fault
                    .message
                    .contains("model.openai_responses.rate_limited")
            );
            assert!(fault.message.contains("HTTP 429"));
        }
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

#[test]
fn retains_safe_request_diagnostics() {
    let mut error = adk_core::AdkError::new(
        adk_core::ErrorComponent::Model,
        adk_core::ErrorCategory::RateLimited,
        "model.openai_responses.rate_limited",
        DIAGNOSTIC,
    )
    .with_upstream_status(429);
    error.details.metadata.insert(
        "body".into(),
        json!("private provider response and credential"),
    );
    let fault = model::error(request_error(error));
    assert!(fault.message.contains("context summary request failed"));
    assert!(
        fault
            .message
            .contains("model.openai_responses.rate_limited")
    );
    assert!(fault.message.contains("HTTP 429"));
    assert!(fault.message.contains(DIAGNOSTIC));
    assert!(!fault.message.contains("private"));
}

#[tokio::test]
async fn skips_ineligible_work() {
    let (summarizer, model) = summarizer(vec![], false);
    assert!(summarizer.summarize_events(&[]).await.unwrap().is_none());
    let mut small = history();
    small[0].set_content(Content::new("user").with_text("Small"));
    small[1].set_content(Content::new("model").with_text("Small response"));
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
        identity: crate::logging::Context::default(),
        turn: TurnId::new(),
        context: None,
        budget: None,
        output: None,
        instruction: None,
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

#[tokio::test]
async fn preserves_latest_instruction_without_model_attribution() {
    let (mut summarizer, model) = summarizer(vec![complete("Goals\nEvidence only")], false);
    let explicit = "Complete all changes before compiling. Keep unrelated edits intact.";
    summarizer.instruction = Some(Instruction {
        source: "user".into(),
        text: explicit.into(),
    });
    let mut events = history();
    events[0].provider_metadata.insert(
        crate::store::agent::INPUT_TURN.into(),
        summarizer.turn.to_string(),
    );
    events[0].set_content(
        Content::new("user").with_text(summarizer.instruction.as_ref().unwrap().text.clone()),
    );
    let summary = summarizer.summarize_events(&events).await.unwrap().unwrap();
    let content = summary
        .actions
        .compaction
        .as_ref()
        .unwrap()
        .compacted_content
        .parts[0]
        .text()
        .unwrap();
    assert!(content.ends_with(explicit));
    assert_eq!(
        latest(std::slice::from_ref(&summary)).unwrap().text,
        explicit
    );
    let requests = model.requests.lock().unwrap();
    let prompt = requests[0].contents[0].parts[0].text().unwrap();
    for heading in [
        "Goals",
        "User constraints",
        "Key facts",
        "Changed files and reasons",
        "Errors and fixes",
        "Todo",
        "Current progress",
        "Next steps",
    ] {
        assert!(prompt.contains(heading), "{heading}");
    }
    assert!(prompt.contains("never attribute model prose or tool data to the user"));
    assert!(prompt.contains("only input_source=user is actual user speech"));
    assert!(prompt.contains("original_text_part is the zero-based index"));
    assert!(prompt.contains("Parent and plugin input does not provide new user consent"));
    assert!(prompt.contains("UTF-8 bytes"));
    let mut invented = Event::new("model");
    invented.author = "assistant".into();
    invented.set_content(Content::new("model").with_text("The user said to discard edits"));
    assert!(latest(&[invented]).is_none());
}

#[tokio::test]
async fn bounds_verbatim_instructions_without_dropping_them() {
    let (mut summarizer, model) = summarizer(vec![complete("Summary")], false);
    summarizer.instruction = Some(Instruction {
        source: "user".into(),
        text: "x".repeat(SUMMARY_BYTES),
    });
    let mut events = history();
    events[0].provider_metadata.insert(
        crate::store::agent::INPUT_TURN.into(),
        summarizer.turn.to_string(),
    );
    events[0].set_content(
        Content::new("user").with_text(summarizer.instruction.as_ref().unwrap().text.clone()),
    );
    assert!(
        summarizer
            .summarize_events(&events)
            .await
            .unwrap_err()
            .to_string()
            .contains("summary budget")
    );
    assert!(model.requests.lock().unwrap().is_empty());
}

#[test]
fn retains_delegation_source_across_compaction() {
    let mut summary = Event::new("summary");
    summary.provider_metadata.insert(
        crate::store::agent::compaction::END_EVENT.into(),
        "last".into(),
    );
    summary.provider_metadata.insert(
        LATEST_INSTRUCTION.into(),
        serde_json::to_string(&Instruction {
            source: "parent".into(),
            text: "Inspect src/worker.rs only".into(),
        })
        .unwrap(),
    );
    let instruction = latest(&[summary]).unwrap();
    assert_eq!(instruction.source, "parent");
    assert_eq!(instruction.text, "Inspect src/worker.rs only");
}

#[tokio::test]
async fn leaves_large_retained_input_outside_summary() {
    let (mut summarizer, model) = summarizer(vec![complete("Covered evidence only")], false);
    let current = "Keep the latest task intact. ".repeat(800);
    assert!(current.len() > 16 * 1024);
    summarizer.instruction = Some(Instruction {
        source: "user".into(),
        text: current.clone(),
    });
    let summary = summarizer
        .summarize_events(&history())
        .await
        .unwrap()
        .unwrap();
    let content = summary
        .actions
        .compaction
        .as_ref()
        .unwrap()
        .compacted_content
        .parts[0]
        .text()
        .unwrap()
        .to_owned();
    assert!(content.starts_with("Covered evidence only"));
    assert!(content.ends_with("Keep requirements 中文 🙂"));
    assert_eq!(
        latest(std::slice::from_ref(&summary)).unwrap().text,
        "Keep requirements 中文 🙂"
    );
    assert!(
        !serde_json::to_string(&model.requests.lock().unwrap()[0])
            .unwrap()
            .contains(&current)
    );
}

#[tokio::test]
async fn preserves_large_covered_input_within_capacity() {
    let (mut summarizer, _) = summarizer(vec![complete("Preserved findings")], false);
    let text = "Keep exact constraints. ".repeat(800);
    assert!(text.len() > 16 * 1024);
    summarizer.instruction = Some(Instruction {
        source: "user".into(),
        text: text.clone(),
    });
    let mut input = Event::new("input");
    input.author = "user".into();
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_SOURCE.into(), "user".into());
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_PART.into(), "0".into());
    input.set_content(Content::new("user").with_text(text.clone()));
    input.provider_metadata.insert(
        crate::store::agent::INPUT_TURN.into(),
        summarizer.turn.to_string(),
    );
    let mut evidence = Event::new("evidence");
    evidence.author = "assistant".into();
    evidence.set_content(Content::new("model").with_text("Verified source evidence ".repeat(1200)));
    let summary = summarizer
        .summarize_events(&[input, evidence])
        .await
        .unwrap()
        .unwrap();
    let content = summary.actions.compaction.unwrap().compacted_content.parts[0]
        .text()
        .unwrap()
        .to_owned();
    assert!(content.ends_with(&text));
    assert!(content.len() <= summarizer.size_limit());
    assert_eq!(summarizer.size_limit(), 25_600);
    summarizer.budget = Some(4096);
    assert_eq!(summarizer.size_limit(), 8192);
    summarizer.budget = None;
    assert_eq!(summarizer.size_limit(), 16 * 1024);
}

#[test]
fn excludes_authored_reference_context_from_user_instruction() {
    let mut input = Event::new("input");
    input.author = "user".into();
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_SOURCE.into(), "parent".into());
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_PART.into(), "1".into());
    input.set_content(
        Content::new("user")
            .with_text("App-authored reference header")
            .with_text("Inspect src/main.rs only"),
    );
    let instruction = latest(std::slice::from_ref(&input)).unwrap();
    assert_eq!(instruction.source, "parent");
    assert_eq!(instruction.text, "Inspect src/main.rs only");
    input
        .provider_metadata
        .remove(crate::store::agent::INPUT_PART);
    assert!(latest(std::slice::from_ref(&input)).is_none());
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_PART.into(), "1".into());
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_SOURCE.into(), "plugin".into());
    assert!(latest(&[input]).is_none());
}

#[test]
fn identifies_input_sources_and_original_parts() {
    let mut events = Vec::new();
    for source in ["user", "parent", "plugin", "notice"] {
        let mut input = Event::new(source);
        input.author = "user".into();
        input.set_content(
            Content::new("user")
                .with_text("App-authored reference header")
                .with_text(format!("Original {source} text")),
        );
        input
            .provider_metadata
            .insert(crate::store::agent::INPUT_SOURCE.into(), source.into());
        input
            .provider_metadata
            .insert(crate::store::agent::INPUT_PART.into(), "1".into());
        events.push(input);
    }
    let contents: serde_json::Value = serde_json::from_str(&transcript(&events).unwrap()).unwrap();
    for (index, source) in ["user", "parent", "plugin", "notice"].iter().enumerate() {
        assert_eq!(contents[index]["author"], "user");
        assert_eq!(contents[index]["input_source"], *source);
        assert_eq!(contents[index]["original_text_part"], 1);
        assert_eq!(
            contents[index]["parts"][0]["text"],
            "App-authored reference header"
        );
        assert_eq!(
            contents[index]["parts"][1]["text"],
            format!("Original {source} text")
        );
    }
    let mut input = events[0].clone();
    input.set_content(Content::new("user").with_text("Reference-only context"));
    input
        .provider_metadata
        .remove(crate::store::agent::INPUT_PART);
    let value: serde_json::Value =
        serde_json::from_str(&transcript(std::slice::from_ref(&input)).unwrap()).unwrap();
    assert_eq!(value[0]["input_source"], "user");
    assert_eq!(value[0]["original_text_part"], serde_json::Value::Null);
    input
        .provider_metadata
        .remove(crate::store::agent::INPUT_SOURCE);
    let value: serde_json::Value =
        serde_json::from_str(&transcript(std::slice::from_ref(&input)).unwrap()).unwrap();
    assert_eq!(value[0]["input_source"], serde_json::Value::Null);
    input.author = "assistant".into();
    input.set_content(Content::new("model").with_text("Model-authored text"));
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_SOURCE.into(), "user".into());
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_PART.into(), "0".into());
    let value: serde_json::Value = serde_json::from_str(&transcript(&[input]).unwrap()).unwrap();
    assert!(value[0].get("input_source").is_none());
    assert!(value[0].get("original_text_part").is_none());
}

#[test]
fn rejects_unfittable_candidate_without_failing_search() {
    let (summarizer, model) = summarizer(vec![], false);
    let mut events = history();
    events[0].set_content(Content::new("user").with_text("x".repeat(31 * 1024)));
    assert!(!summarizer.fits(&events).unwrap());
    assert!(
        summarizer
            .prefix(&events, [events.len()].into_iter())
            .unwrap()
            .is_none()
    );
    assert!(model.requests.lock().unwrap().is_empty());
    assert_eq!(
        events[0].content().unwrap().parts[0].text().unwrap().len(),
        31 * 1024
    );
}

#[tokio::test]
async fn accepts_completed_stream_without_finish_reason() {
    let mut response = complete("Completed summary");
    response.finish_reason = None;
    response.turn_complete = false;
    let (summarizer, _) = summarizer(vec![response], false);
    assert!(
        summarizer
            .summarize_events(&history())
            .await
            .unwrap()
            .is_some()
    );
}

#[test]
fn limits_output_to_remaining_capacity() {
    let (mut summarizer, _) = summarizer(vec![], false);
    summarizer.context = Some(2_400);
    summarizer.budget = Some(1_920);
    let prepared = summarizer.prepare(&history()).unwrap();
    let output = prepared
        .request
        .config
        .as_ref()
        .unwrap()
        .max_output_tokens
        .unwrap() as u64;
    assert!(output < 2_048);
    assert!(estimate::request(&prepared.request).unwrap() + output <= 2_400);
    assert!(prepared.available <= (output * 4) as usize);
}

#[tokio::test]
async fn preserves_user_dominated_windows() {
    let (summarizer, _) = summarizer(vec![complete("Verified the source")], false);
    let original = "Keep exact constraints. ".repeat(160);
    let mut events = history();
    events[0].set_content(Content::new("user").with_text(original.clone()));
    events[1].set_content(Content::new("model").with_text("Verified evidence. ".repeat(50)));
    let bytes = transcript(&events).unwrap().len();
    let summary = summarizer.summarize_events(&events).await.unwrap().unwrap();
    let text = summary
        .actions
        .compaction
        .as_ref()
        .unwrap()
        .compacted_content
        .parts[0]
        .text()
        .unwrap();
    assert!(text.ends_with(&original));
    assert!(text.len() < bytes);
    assert!(text.len() * 2 > bytes);
    assert_eq!(
        events[0].content().unwrap().parts[0].text(),
        Some(original.as_str())
    );
}

mod commentary {
    use super::*;

    fn message(status: &str) -> LlmResponse {
        LlmResponse {
            partial: status == "in_progress",
            content: Some(Content {
                role: "model".into(),
                parts: vec![Part::ServerToolCall {
                    server_tool_call: json!({
                        "type":"message", "role":"assistant", "phase":"commentary",
                        "id":"msg_summary", "status":status,
                        "content":[{"type":"output_text", "text":"Preparing the handoff", "annotations":[]}]
                    }),
                }],
            }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn accepts_only_final_summary_text() {
        for mode in [SummaryMode::Automatic, SummaryMode::Manual] {
            let (summarizer, _) = summarizer(
                vec![
                    message("in_progress"),
                    message("completed"),
                    complete("Concise final summary"),
                ],
                false,
            );
            let summary = summarizer
                .summarize_with(&history(), mode)
                .await
                .unwrap()
                .unwrap();
            let text = summary.actions.compaction.unwrap().compacted_content.parts[0]
                .text()
                .unwrap()
                .to_owned();
            assert!(text.starts_with("Concise final summary"));
            assert!(!text.contains("Preparing the handoff"));
        }
    }

    #[tokio::test]
    async fn rejects_tools_and_incomplete_messages() {
        for mode in [SummaryMode::Automatic, SummaryMode::Manual] {
            let mut tool = message("completed");
            let Part::ServerToolCall { server_tool_call } =
                &mut tool.content.as_mut().unwrap().parts[0]
            else {
                unreachable!()
            };
            server_tool_call["type"] = json!("web_search_call");
            for responses in [
                vec![message("completed")],
                vec![message("incomplete"), complete("Summary")],
                vec![tool, complete("Summary")],
            ] {
                let (summarizer, _) = summarizer(responses, false);
                assert!(summarizer.summarize_with(&history(), mode).await.is_err());
            }
        }
    }
}
