use super::*;
use goose_provider_types::{
    base::{MessageStream, stream_from_single_message},
    conversation::token_usage::{CostSource, ProviderStats, ProviderUsage, Usage},
    errors::ProviderError,
};
use rmcp::model::{
    Annotations, CallToolRequestParams, CallToolResult, ContentBlock, Role, TextContent, Tool,
};
use tokio::sync::Notify;

#[derive(Default)]
struct Fixture {
    managed: bool,
    fail: Option<usize>,
    pause: Option<usize>,
    entered: Notify,
    requests: Mutex<Vec<(ModelConfig, String)>>,
}

#[async_trait]
impl Provider for Fixture {
    fn get_name(&self) -> &str {
        "fixture"
    }

    fn manages_own_context(&self) -> bool {
        self.managed
    }

    async fn stream(
        &self,
        config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        assert!(system.starts_with("Your task is to summarize a tool call & response pair"));
        assert!(tools.is_empty());
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::User);
        let index = {
            let mut requests = self.requests.lock().unwrap();
            requests.push((config.clone(), messages[0].as_concat_text()));
            requests.len()
        };
        if self.pause == Some(index) {
            self.entered.notify_one();
            std::future::pending::<()>().await;
        }
        if self.fail == Some(index) {
            return Err(ProviderError::RequestFailed("summary unavailable".into()));
        }
        Ok(stream_from_single_message(
            Message::assistant().with_text(format!("Summary {index}")),
            ProviderUsage::new(
                "fixture".into(),
                Usage {
                    input_tokens: Some(12),
                    output_tokens: Some(10),
                    total_tokens: Some(22),
                    cache_read_input_tokens: Some(3),
                    ..Default::default()
                },
            )
            .with_cost(0.01, CostSource::ProviderReported)
            .with_stats(ProviderStats {
                elapsed_ms: Some(2),
                time_to_first_token_ms: Some(1),
                ..Default::default()
            }),
        ))
    }
}

fn conversation(groups: usize, width: usize) -> Conversation {
    let mut messages = vec![Message::user().with_generated_id().with_text("Old work")];
    for group in 0..groups {
        let mut request = Message::assistant().with_generated_id();
        let mut response = Message::user().with_generated_id();
        for slot in 0..width {
            let id = format!("call-{group}-{slot}");
            request =
                request.with_tool_request(id.clone(), Ok(CallToolRequestParams::new("read_file")));
            response = response.with_tool_response(
                id,
                Ok(CallToolResult::success(vec![ContentBlock::text(format!(
                    "Exact result {group}-{slot}"
                ))])),
            );
        }
        response.created = group as i64 + 1;
        messages.extend([request, response]);
    }
    Conversation::new_unvalidated(messages)
}

fn session(conversation: &Conversation) -> Session {
    Session {
        id: "session".into(),
        records: Vec::new(),
        conversation: conversation.clone(),
        context: Default::default(),
        capability: None,
        reviewed: false,
        state: Default::default(),
        thinking: None,
        invocation: None,
    }
}

fn emitter() -> Emitter {
    let (sender, _) = tokio::sync::mpsc::unbounded_channel();
    Emitter::new(sender, sailry_link::CancellationToken::new())
}

fn operation(provider: Arc<Fixture>) -> ToolPairs {
    ToolPairs::new(
        provider,
        ModelConfig::new("fixture").with_thinking_effort(ThinkingEffort::High),
        TurnId::new(),
        10_000,
        0.8,
    )
}

fn effects(result: OperationResult<Effect>) -> Vec<Effect> {
    let OperationResult::Applied(result) = result else {
        panic!("tool summaries expected")
    };
    result.effects
}

#[test]
fn scales_cutoff_with_context() {
    assert_eq!(cutoff(128_000, 0.8), 15);
    assert_eq!(cutoff(1_000_000, 0.8), 120);
    assert_eq!(cutoff(50_000, 0.8), 10);
    assert_eq!(cutoff(10_000_000, 0.8), 500);
    assert_eq!(cutoff(200_000, 0.0), 24);
}

#[tokio::test]
async fn protects_current_kickoff_and_batches_old_pairs() {
    let provider = Arc::new(Fixture::default());
    let operation = operation(provider.clone());
    let emit = emitter();
    let mut history = conversation(21, 1);
    assert!(matches!(
        operation
            .run(&session(&history), &history, &emit)
            .await
            .unwrap(),
        OperationResult::NotApplicable
    ));
    let mut boundary = conversation(20, 1);
    boundary.push(Message::user().with_generated_id().with_text("Next task"));
    assert!(matches!(
        operation
            .run(&session(&boundary), &boundary, &emit)
            .await
            .unwrap(),
        OperationResult::NotApplicable
    ));
    history.push(Message::user().with_generated_id().with_text("Next task"));
    let original = history.clone();
    let started = chrono::Utc::now();
    let effects = effects(
        operation
            .run(&session(&history), &history, &emit)
            .await
            .unwrap(),
    );
    assert_eq!(history, original);
    assert_eq!(effects.len(), 4 * BATCH);
    for (index, group) in effects.as_chunks::<4>().0.iter().enumerate() {
        for (effect, source) in group[..2].iter().zip(&history.messages()[1 + index * 2..]) {
            let Effect::Conversation(ConversationEffect::SetMessageVisibility {
                message_id,
                user_visible,
                agent_visible,
            }) = effect
            else {
                panic!("visibility effect expected")
            };
            assert_eq!(Some(message_id), source.id.as_ref());
            assert!(*user_visible);
            assert!(!*agent_visible);
        }
        let Effect::Conversation(ConversationEffect::AppendMessage(summary)) = &group[2] else {
            panic!("summary effect expected")
        };
        assert_eq!(summary.role, Role::User);
        assert_eq!(summary.created, index as i64 + 1);
        assert_eq!(summary.as_concat_text(), format!("Summary {}", index + 1));
        assert!(!summary.is_user_visible());
        assert!(summary.is_agent_visible());
        assert!(summary.id.is_some());
        assert_eq!(
            summary
                .metadata
                .operation_note("tool_pair_compaction", "after"),
            Some(&serde_json::json!(history.messages()[2 + index * 2].id)),
        );
        assert!(summary.metadata.usage.is_none());
        let Effect::Records(records) = &group[3] else {
            panic!("usage record expected")
        };
        assert_eq!(records.len(), 1);
        let charge = &records[0];
        assert_eq!(charge.turn, operation.turn);
        assert_eq!(charge.provenance.author, "system");
        assert!(charge.created >= started && charge.created <= chrono::Utc::now());
        assert!(charge.created.timestamp() > summary.created);
        let message = charge.message().unwrap();
        assert!(message.content.is_empty());
        assert_eq!(message.model.as_deref(), Some("fixture"));
        let usage = message.usage.as_ref().unwrap();
        assert_eq!(
            (usage.input, usage.output, usage.total),
            (Some(12), Some(10), Some(22))
        );
        assert_eq!(usage.cache_read, Some(3));
        assert_eq!(usage.cost, Some(0.01));
        let runtime = charge.metadata.runtime.as_ref().unwrap();
        assert!(!runtime.agent_visible && !runtime.user_visible);
        let execution = charge.metadata.execution.as_ref().unwrap();
        assert_eq!(execution.purpose, Some(canonical::Purpose::Compaction));
        assert!(execution.context_tokens.is_none());
        let timing = execution.timing.as_ref().unwrap();
        assert_eq!(
            (timing.elapsed_us, timing.first_token_us),
            (Some(2000), Some(1000))
        );
    }
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), BATCH);
    assert!(requests.iter().all(|(config, _)| {
        config.thinking_effort() == Some(ThinkingEffort::Off) && config.prompt_cache_disabled()
    }));
    assert!(!requests[0].1.contains("Exact result 10-0"));
}

#[tokio::test]
async fn formats_file_text_before_summarizing() {
    let provider = Arc::new(Fixture::default());
    let operation = operation(provider.clone());
    let body = format!("head marker {}UNSHOWN_LINE_TAIL\r\n", "中".repeat(3000));
    let raw = serde_json::json!({"kind":"file_lines","data":sailry_protocol::FileLines {
        path:"src/fixture.rs".into(),
        lines:vec![
            sailry_protocol::FileLine {number:1,text:body.clone()},
            sailry_protocol::FileLine {number:2,text:"last line".into()},
        ],
        size:(body.len()+"last line".len()) as u64,
        revision:"current-file-revision".into(),offset:0,next_offset:None,
    }});
    let request = Message::assistant()
        .with_id("source-request")
        .with_tool_request("file", Ok(CallToolRequestParams::new("read_file")));
    let mut response = Message::user()
        .with_id("source-response")
        .with_tool_response("file", Ok(CallToolResult::structured(raw.clone())));
    response.created = 42;
    let history = Conversation::new_unvalidated([request, response]);
    let original = history.clone();
    let (summary, _) = operation.summarize(&history, "file").await.unwrap();
    assert_eq!(history, original);
    assert_eq!(summary.created, 42);
    assert_eq!(
        summary
            .metadata
            .operation_note("tool_pair_compaction", "after"),
        Some(&serde_json::json!("source-response"))
    );
    let requests = provider.requests.lock().unwrap();
    let input = &requests[0].1;
    assert!(input.contains("src/fixture.rs\nLines 1–2;"));
    assert!(input.contains("head marker"));
    assert!(input.contains("content bytes omitted from this line"));
    assert!(input.contains("last line [no trailing newline]"));
    assert!(input.contains("read_tool_output"));
    assert!(
        input.contains(
            &blake3::hash(raw.to_string().as_bytes())
                .to_hex()
                .to_string()
        )
    );
    assert!(!input.contains("UNSHOWN_LINE_TAIL"));
    assert!(!input.contains("\"lines\":["));
}

#[tokio::test]
async fn summarizes_parallel_siblings_once() {
    let provider = Arc::new(Fixture::default());
    let operation = operation(provider.clone());
    let mut history = conversation(11, 2);
    history.push(Message::user().with_generated_id().with_text("Next task"));
    let result = effects(
        operation
            .run(&session(&history), &history, &emitter())
            .await
            .unwrap(),
    );
    assert_eq!(result.len(), 4 * BATCH / 2);
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), BATCH / 2);
    assert!(requests[0].1.contains("Exact result 0-0"));
    assert!(requests[0].1.contains("Exact result 0-1"));
}

#[tokio::test]
async fn preserves_unsummarized_failures() {
    let provider = Arc::new(Fixture {
        fail: Some(1),
        ..Default::default()
    });
    let operation = operation(provider.clone());
    let mut history = conversation(21, 1);
    history.push(Message::user().with_generated_id().with_text("Next task"));
    let result = effects(
        operation
            .run(&session(&history), &history, &emitter())
            .await
            .unwrap(),
    );
    assert_eq!(result.len(), 4 * (BATCH - 1));
    assert_eq!(provider.requests.lock().unwrap().len(), BATCH);
    for effect in result {
        if let Effect::Conversation(ConversationEffect::SetMessageVisibility {
            message_id, ..
        }) = effect
        {
            assert_ne!(Some(&message_id), history.messages()[1].id.as_ref());
            assert_ne!(Some(&message_id), history.messages()[2].id.as_ref());
        }
    }
}

#[tokio::test]
async fn drains_completed_summaries_after_cancellation() {
    let provider = Arc::new(Fixture {
        pause: Some(2),
        ..Default::default()
    });
    let operation = operation(provider.clone());
    let mut history = conversation(21, 1);
    history.push(Message::user().with_generated_id().with_text("Next task"));
    let session = session(&history);
    let emit = emitter();
    let mut execution = Box::pin(operation.run(&session, &history, &emit));
    tokio::select! {
        _=provider.entered.notified()=>{},
        result=&mut execution=>panic!("summary should remain pending: {}",result.is_ok()),
    }
    drop(execution);
    let result = operation
        .finalize_cancellation(&session, &history, &emit)
        .await;
    assert_eq!(result.len(), 4);
    assert!(
        matches!(&result[2], Effect::Conversation(ConversationEffect::AppendMessage(message)) if message.as_concat_text()=="Summary 1")
    );
    assert!(
        matches!(&result[3], Effect::Records(records) if records.len()==1 && records[0].usage().unwrap().input==Some(12))
    );
    assert!(
        operation
            .finalize_cancellation(&session, &history, &emit)
            .await
            .is_empty()
    );
    assert_eq!(provider.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn excludes_provider_managed_context() {
    let provider = Arc::new(Fixture {
        managed: true,
        ..Default::default()
    });
    let operation = operation(provider.clone());
    let mut history = conversation(21, 1);
    history.push(Message::user().with_generated_id().with_text("Next task"));
    assert!(matches!(
        operation
            .run(&session(&history), &history, &emitter())
            .await
            .unwrap(),
        OperationResult::NotApplicable
    ));
    assert!(provider.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn respects_nested_content_audiences() {
    let provider = Arc::new(Fixture::default());
    let operation = operation(provider.clone());
    let mut response = Message::user().with_generated_id().with_tool_response(
        "call",
        Ok(CallToolResult::success(vec![
            ContentBlock::text("Visible result"),
            ContentBlock::Text(
                TextContent::new("Private user content")
                    .with_annotations(Annotations::default().with_audience(vec![Role::User])),
            ),
        ])),
    );
    let request = Message::assistant()
        .with_generated_id()
        .with_tool_request("call", Ok(CallToolRequestParams::new("read_file")));
    operation
        .summarize(
            &Conversation::new_unvalidated([request.clone(), response.clone()]),
            "call",
        )
        .await
        .unwrap();
    let prompt = provider.requests.lock().unwrap()[0].1.clone();
    assert!(prompt.contains("Visible result"));
    assert!(!prompt.contains("Private user content"));
    response.metadata = MessageMetadata::user_only();
    assert!(
        operation
            .summarize(&Conversation::new_unvalidated([request, response]), "call")
            .await
            .is_err()
    );
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
}
