use super::*;
use crate::store::agent::Invocation;
use goose_agent::machine::{StateMachine, Step};
use goose_provider_types::{
    base::MessageStream,
    conversation::{message::MessageUsage, token_usage::Usage},
};
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use sailry_link::CancellationToken;
use sailry_protocol::{conversation::RunKind, *};
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Default)]
struct Managed {
    requests: AtomicUsize,
}

#[async_trait]
impl Provider for Managed {
    fn get_name(&self) -> &str {
        "managed-fixture"
    }

    fn manages_own_context(&self) -> bool {
        true
    }

    async fn stream(
        &self,
        _: &ModelConfig,
        _: &str,
        _: &[Message],
        _: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        Err(ProviderError::RequestFailed(
            "unexpected summary request".into(),
        ))
    }
}

/// One scripted summary response: text, output-limit flag and stop reason.
struct Reply {
    text: String,
    limited: bool,
    finish: &'static str,
    usage: Usage,
}

#[derive(Default)]
struct Scripted {
    replies: Mutex<VecDeque<Reply>>,
    systems: Mutex<Vec<String>>,
    requests: Mutex<Vec<Vec<Message>>>,
}

impl Scripted {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into_iter().collect()),
            systems: Mutex::default(),
            requests: Mutex::default(),
        })
    }
}

#[async_trait]
impl Provider for Scripted {
    fn get_name(&self) -> &str {
        "scripted-fixture"
    }

    async fn stream(
        &self,
        _: &ModelConfig,
        system: &str,
        messages: &[Message],
        _: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        self.systems.lock().unwrap().push(system.into());
        self.requests.lock().unwrap().push(messages.to_vec());
        let reply = self.replies.lock().unwrap().pop_front().unwrap();
        let id = "summary-response";
        let mut chunks = vec![Ok((
            Some(Message::assistant().with_id(id).with_text(reply.text)),
            None,
        ))];
        if reply.limited {
            let mut limit = Message::assistant().with_id(id);
            limit.metadata.output_token_limit_reached = true;
            chunks.push(Ok((Some(limit), None)));
        }
        chunks.push(Ok((
            None,
            Some(
                ProviderUsage::new("scripted-fixture".into(), reply.usage)
                    .with_finish_reasons(vec![reply.finish.into()]),
            ),
        )));
        Ok(Box::pin(futures::stream::iter(chunks)))
    }
}

/// Never answers; records when the request stream is dropped.
#[derive(Default)]
struct Pending {
    started: tokio::sync::Notify,
    dropped: Arc<AtomicBool>,
}

struct Guard(Arc<AtomicBool>);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl Provider for Pending {
    fn get_name(&self) -> &str {
        "pending-fixture"
    }

    async fn stream(
        &self,
        _: &ModelConfig,
        _: &str,
        _: &[Message],
        _: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        self.started.notify_one();
        let guard = Guard(self.dropped.clone());
        Ok(Box::pin(futures::stream::unfold(
            guard,
            |guard| async move {
                std::future::pending::<()>().await;
                Some((Ok((None, None)), guard))
            },
        )))
    }
}

fn structured(fields: serde_json::Value) -> String {
    format!("<analysis>Checklist</analysis>\n```json\n{fields}\n```")
}

fn reply(text: String) -> Reply {
    Reply {
        text,
        limited: false,
        finish: "stop",
        usage: Usage::default(),
    }
}

fn model(provider: Arc<dyn Provider>) -> Model {
    Model {
        provider,
        config: ModelConfig::new("scripted-fixture"),
        files: Mutex::default(),
    }
}

async fn summarize(model: &Model, messages: Vec<Message>) -> anyhow::Result<String> {
    let messages = super::input::prepare(&messages, 64_000, 8_000)?;
    let (summary, _) = super::summarize::run(model, &messages).await?;
    Ok(summary.as_concat_text())
}

fn invocation(node: &crate::Node) -> Arc<Invocation> {
    Arc::new(Invocation {
        compaction_percent: 80,
        child_concurrency: sailry_protocol::team::CHILD_CONCURRENCY,
        instructions: vec![],
        hooks: Default::default(),
        automatic: false,
        notice: false,
        notice_input: None,
        notices: vec![],
        title: None,
        connections: crate::store::agent::connections::Catalog {
            bound: None,
            databases: vec![],
            ssh: vec![],
        },
        turn: QueuedTurn {
            id: TurnId::new(),
            kind: RunKind::Compaction,
            session: SessionId::new(),
            request: RequestId::new(),
            revision: 1,
            config: SessionConfig {
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "managed-fixture".into(),
                effort: Effort::Low,
                features: Default::default(),
                mode: WorkMode::Code,
                options: Default::default(),
                permission: Permission::Ask,
                credential: None,
            },
            roles: Default::default(),
            plugins: vec![],
        },
        caller: node.id(),
        worktree: WorktreeId::new(),
        project: None,
        provider: None,
        media: Default::default(),
        message: Default::default(),
        input_turn: None,
        followups: vec![],
        child: None,
        plugins: vec![],
        plugin_settings: Default::default(),
    })
}

fn preparer(
    node: &crate::Node,
    invocation: &Arc<Invocation>,
    stop: &CancellationToken,
) -> Arc<super::super::prompt::Preparer> {
    Arc::new(super::super::prompt::Preparer {
        ingress: node.ingress.clone(),
        invocation: invocation.clone(),
        stop: stop.clone(),
        deferred: Default::default(),
        images: vec![],
        parts: vec![],
        sources: Default::default(),
        reported: Default::default(),
    })
}

#[tokio::test]
async fn yields_managed_context_after_previous_error() {
    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let stop = CancellationToken::new();
    let invocation = invocation(&node);
    let preparer = preparer(&node, &invocation, &stop);
    let previous = TurnId::new();
    let session = super::super::tests::session(vec![(
        previous,
        Message::assistant()
            .with_generated_id()
            .with_error(MessageErrorKind::Authentication, "Previous request failed"),
    )]);
    assert!(super::super::last_error(&session, previous).is_some());
    let provider = Arc::new(Managed::default());
    let (sender, _) = tokio::sync::mpsc::unbounded_channel();
    let emit = Emitter::new(sender, stop.clone());
    for manual in [false, true] {
        let operation = Compaction {
            provider: provider.clone(),
            config: ModelConfig::new("managed-fixture"),
            context: 64_000,
            threshold: 0.8,
            preparer: preparer.clone(),
            manual,
        };
        let machine = StateMachine::new(vec![Step::Operation(Arc::new(operation))], stop.clone());
        let result = machine.step(&session, &emit).await.unwrap();
        assert_eq!(result.status, RunStatus::Yielded);
        assert!(result.applied_step.is_none());
        assert!(result.effects.is_empty());
        assert!(super::super::last_error(&session, invocation.turn.id).is_none());
        assert!(super::super::output(&session, invocation.turn.id).is_none());
    }
    assert_eq!(provider.requests.load(Ordering::SeqCst), 0);
    node.shutdown().await.unwrap();
}

/// One call and its result, visible to the agent unless `hidden`.
fn exchange(id: &str, name: &str, result: CallToolResult, hidden: bool) -> [Message; 2] {
    [
        Message::assistant()
            .with_generated_id()
            .with_tool_request(id, Ok(CallToolRequestParams::new(name.to_owned()))),
        Message::user()
            .with_generated_id()
            .with_tool_response(id, Ok(result)),
    ]
    .map(|message| {
        if hidden {
            message.with_visibility(false, false)
        } else {
            message
        }
    })
}

fn discovery(name: &str) -> CallToolResult {
    CallToolResult::structured(serde_json::json!({
        "tools":[{"name":name,"description":"Fixture"}],
        "sailry_context":"",
    }))
}

#[test]
fn keeps_tools_used_since_compaction() {
    let messages: Vec<_> = [
        exchange("old", "alpha", CallToolResult::success(vec![]), true),
        exchange("search", "tool_search", discovery("gamma"), false),
        exchange("call", "beta", CallToolResult::success(vec![]), false),
    ]
    .into_iter()
    .flatten()
    .collect();
    let loaded = ["alpha", "beta", "delta", "gamma"].map(String::from);
    assert_eq!(
        loadout::retained(&loaded, &Conversation::new_unvalidated(messages)),
        ["beta", "gamma"]
    );
}

#[tokio::test]
async fn drops_stale_tools_and_summaries() {
    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let stop = CancellationToken::new();
    let invocation = invocation(&node);
    let mut earlier = Message::user()
        .with_generated_id()
        .with_text("Earlier handoff")
        .with_visibility(false, true);
    earlier.metadata.usage = Some(Box::new(MessageUsage {
        is_compaction: true,
        ..Default::default()
    }));
    let messages = exchange("old", "alpha", CallToolResult::success(vec![]), true)
        .into_iter()
        .chain([earlier])
        .chain(exchange("search", "tool_search", discovery("gamma"), false))
        .chain(exchange(
            "call",
            "beta",
            CallToolResult::success(vec![]),
            false,
        ))
        .map(|message| (invocation.turn.id, message))
        .collect();
    let mut session = super::super::tests::session(messages);
    session.context.loaded_tools = Some(
        ["alpha", "beta", "delta", "gamma"]
            .map(String::from)
            .to_vec(),
    );
    let operation = Compaction {
        provider: Scripted::new([reply(structured(
            serde_json::json!({"user_intent":["Fix the parser"]}),
        ))]),
        config: ModelConfig::new("scripted-fixture"),
        context: 64_000,
        threshold: 0.8,
        preparer: preparer(&node, &invocation, &stop),
        manual: false,
    };
    let effects = operation
        .compact(&session, &session.conversation, "summary-fixture")
        .await
        .unwrap();
    // No carried input: the earlier handoff is merged, never repeated verbatim.
    let [Effect::Compaction { summary, tools, .. }, _] = effects.as_slice() else {
        panic!("summary and continuation expected")
    };
    assert!(summary.as_concat_text().contains("Fix the parser"));
    assert_eq!(tools, &["beta", "gamma"]);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn keeps_tools_called_earlier_in_turn() {
    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let stop = CancellationToken::new();
    let invocation = invocation(&node);
    let previous = TurnId::new();
    let earlier = exchange("old", "alpha", CallToolResult::success(vec![]), true)
        .map(|message| (previous, message));
    let current = std::iter::once(
        Message::user()
            .with_generated_id()
            .with_text("Use beta on every source file ".repeat(200)),
    )
    .chain(exchange(
        "call",
        "beta",
        CallToolResult::success(vec![]),
        false,
    ))
    .map(|message| (invocation.turn.id, message));
    let mut session = super::super::tests::session(earlier.into_iter().chain(current).collect());
    session.context.loaded_tools = Some(["alpha", "beta"].map(String::from).to_vec());
    let operation = Compaction {
        provider: Scripted::new([reply(structured(
            serde_json::json!({"user_intent":["Use beta on every source file"]}),
        ))]),
        config: ModelConfig::new("scripted-fixture"),
        context: 64_000,
        threshold: 0.01,
        preparer: preparer(&node, &invocation, &stop),
        manual: false,
    };
    let (sender, _) = tokio::sync::mpsc::unbounded_channel();
    let emit = Emitter::new(sender, stop.clone());
    // The turn continues after its tool result, so compaction runs mid-turn.
    let OperationResult::Applied(result) = operation
        .run(&session, &session.conversation, &emit)
        .await
        .unwrap()
    else {
        panic!("automatic compaction expected")
    };
    let tools = result.effects.iter().find_map(|effect| match effect {
        Effect::Compaction { tools, .. } => Some(tools.clone()),
        _ => None,
    });
    assert_eq!(tools.unwrap(), ["beta"]);
    node.shutdown().await.unwrap();
}

mod summary {
    use super::*;

    #[derive(Default)]
    struct Overflowing {
        requests: AtomicUsize,
        system: Mutex<Option<String>>,
    }

    #[async_trait]
    impl Provider for Overflowing {
        fn get_name(&self) -> &str {
            "overflow-fixture"
        }

        async fn stream(
            &self,
            _: &ModelConfig,
            system: &str,
            _: &[Message],
            _: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            *self.system.lock().unwrap() = Some(system.to_owned());
            Err(ProviderError::ContextLengthExceeded(
                "summary context exceeded".to_owned(),
            ))
        }
    }

    fn history() -> Vec<Message> {
        vec![Message::user().with_text("Fix the parser and keep the v1 wire format")]
    }

    #[tokio::test]
    async fn preserves_latest_tools_on_overflow() {
        let provider = Arc::new(Overflowing::default());
        let messages = history()
            .into_iter()
            .chain(exchange(
                "read",
                "read_file",
                CallToolResult::success(vec![rmcp::model::ContentBlock::text(
                    "Latest exact evidence",
                )]),
                false,
            ))
            .collect();
        let error = summarize(&model(provider.clone()), messages)
            .await
            .unwrap_err();
        assert_eq!(provider.requests.load(Ordering::SeqCst), 1);
        assert!(
            provider
                .system
                .lock()
                .unwrap()
                .as_deref()
                .unwrap()
                .contains("Latest exact evidence")
        );
        assert!(
            error
                .to_string()
                .contains("there are no tool responses to remove")
        );
    }

    #[tokio::test]
    async fn rejects_truncated_output() {
        let valid = structured(serde_json::json!({"user_intent":["Fix the parser"]}));
        for (limited, finish) in [(true, "stop"), (false, "length"), (false, "max_tokens")] {
            let provider = Scripted::new([Reply {
                text: valid.clone(),
                limited,
                finish,
                usage: Usage::default(),
            }]);
            let error = summarize(&model(provider), history()).await.unwrap_err();
            assert!(error.to_string().contains("output limit"), "{error}");
        }
    }

    #[tokio::test]
    async fn rejects_unstructured_output() {
        for text in [
            "Here is a prose summary of the work so far.".to_owned(),
            "```json\n{\"user_intent\": [\"Fix the parser\"], \"pending_tasks\": [\"Write".into(),
        ] {
            let provider = Scripted::new([reply(text)]);
            let error = summarize(&model(provider), history()).await.unwrap_err();
            assert!(error.to_string().contains("structured handoff"), "{error}");
        }
    }

    #[tokio::test]
    async fn uses_concise_handoff_prompt() {
        let provider = Scripted::new([reply(structured(
            serde_json::json!({"user_intent":["Fix the parser"]}),
        ))]);
        summarize(&model(provider.clone()), history())
            .await
            .unwrap();
        let system = provider.systems.lock().unwrap()[0].clone();
        assert!(system.contains("Fix the parser and keep the v1 wire format"));
        assert!(system.contains("merge it with the newer progress"));
        assert!(system.contains("usually no more than 15"));
        assert!(system.contains("under 3,000 tokens"));
        assert!(!system.contains("{{ messages }}"));
    }

    #[tokio::test]
    async fn estimates_usage_before_rendering() {
        let document = structured(serde_json::json!({"user_intent":["Fix the parser"]}));
        let normalized =
            serde_json::to_string(&super::super::structured::Summary::parse(&document).unwrap())
                .unwrap();
        let provider = Scripted::new([reply(document)]);
        let messages = super::super::input::prepare(&history(), 64_000, 8_000).unwrap();
        let (summary, usage) = super::super::summarize::run(&model(provider.clone()), &messages)
            .await
            .unwrap();
        let system = provider.systems.lock().unwrap()[0].clone();
        let request = provider.requests.lock().unwrap()[0].clone();
        assert_eq!(request.len(), 1);
        assert_eq!(
            request[0].as_concat_text(),
            "Please summarize the conversation history provided in the system prompt."
        );
        let expected_input = tokens::chat(&system, &request) as i32;
        let expected_output = tokens::text(&MessageContent::text(normalized).to_string()) as i32;
        assert_eq!(usage.usage.input_tokens, Some(expected_input));
        assert_eq!(usage.usage.output_tokens, Some(expected_output));
        assert_eq!(
            usage.usage.total_tokens,
            Some(expected_input + expected_output)
        );
        assert_ne!(
            expected_output,
            tokens::text(&summary.as_concat_text()) as i32
        );
        assert_eq!(summary.role, rmcp::model::Role::User);
    }

    #[tokio::test]
    async fn preserves_reported_usage() {
        for (input, output) in [(Some(123), None), (None, Some(45)), (Some(123), Some(45))] {
            let provider = Scripted::new([Reply {
                usage: Usage {
                    input_tokens: input,
                    output_tokens: output,
                    total_tokens: Some(999),
                    ..Usage::default()
                },
                ..reply(structured(
                    serde_json::json!({"user_intent":["Fix the parser"]}),
                ))
            }]);
            let messages = super::super::input::prepare(&history(), 64_000, 8_000).unwrap();
            let (_, usage) = super::super::summarize::run(&model(provider), &messages)
                .await
                .unwrap();
            if let Some(input) = input {
                assert_eq!(usage.usage.input_tokens, Some(input));
            }
            if let Some(output) = output {
                assert_eq!(usage.usage.output_tokens, Some(output));
            }
            assert_eq!(
                usage.usage.total_tokens,
                Some(usage.usage.input_tokens.unwrap() + usage.usage.output_tokens.unwrap())
            );
        }
    }

    #[tokio::test]
    async fn bounds_oversized_handoff() {
        let essential = serde_json::json!({
            "user_intent":["Fix the parser"],
            "user_messages":["Keep the v1 wire format"],
            "pending_tasks":["Add a regression test"],
            "next_step":"Run the parser tests",
        });
        let mut references = essential.clone();
        references["current_work"] = "Parser fix modified".into();
        references["technical_concepts"] = (0..400)
            .map(|index| format!("Concept {index}: {}", "detail ".repeat(8)))
            .collect();
        let mut verbose = essential;
        verbose["current_work"] = "状态更新 ".repeat(4_000).into();
        let provider = Scripted::new([reply(structured(references)), reply(structured(verbose))]);
        let model = model(provider);
        let trimmed = summarize(&model, history()).await.unwrap();
        let shortened = summarize(&model, history()).await.unwrap();
        for text in [&trimmed, &shortened] {
            let size = tokens::text(text);
            assert!(size <= handoff::TOKENS, "{size}");
            for kept in [
                "- Fix the parser",
                "- Keep the v1 wire format",
                "- Add a regression test",
                "Run the parser tests",
            ] {
                assert!(text.contains(kept), "{kept}");
            }
        }
        // Lists lose their least important tails first.
        assert!(trimmed.contains("Concept 0: "));
        assert!(!trimmed.contains("Concept 399: "));
        assert!(shortened.contains("状态更新"));
    }

    #[tokio::test]
    async fn keeps_constraints_and_tasks() {
        let state = serde_json::json!({
            "user_intent":["修复解析器"],
            "user_messages":["保持 v1 协议格式", "不要修改公共接口"],
            "pending_tasks":["添加回归测试", "更新架构文档"],
            "problem_solving":["边界检查放在词法阶段"],
            "current_work":"修改解析器的边界检查，".repeat(600),
            "next_step":"运行解析器测试并检查输出，".repeat(600),
        });
        let mut crowded = state.clone();
        crowded["user_messages"] = (0..40)
            .map(|index| format!("约束 {index}：{}", "保持输出稳定并记录原因，".repeat(20)))
            .collect();
        crowded["pending_tasks"] = (0..40)
            .map(|index| format!("任务 {index}：{}", "补充测试并核对结果，".repeat(20)))
            .collect();
        let provider = Scripted::new([reply(structured(state)), reply(structured(crowded))]);
        let model = model(provider);
        let shortened = summarize(&model, history()).await.unwrap();
        let crowded = summarize(&model, history()).await.unwrap();
        for text in [&shortened, &crowded] {
            let size = tokens::text(text);
            assert!(size <= handoff::TOKENS, "{size}");
            assert!(text.contains("- 修复解析器"));
            assert!(text.contains("修改解析器的边界检查"));
            assert!(text.contains("运行解析器测试"));
        }
        // Long state is shortened before any constraint or task is dropped.
        for kept in [
            "- 保持 v1 协议格式",
            "- 不要修改公共接口",
            "- 添加回归测试",
            "- 更新架构文档",
        ] {
            assert!(shortened.contains(kept), "{kept}");
        }
        assert!(!shortened.contains("边界检查放在词法阶段"));
        // The first constraint and task survive even when their lists overflow.
        assert!(crowded.contains("- 约束 0："));
        assert!(crowded.contains("- 任务 0："));
        assert!(!crowded.contains("任务 39："));
    }

    #[tokio::test]
    async fn carries_handoff_across_compactions() {
        let provider = Scripted::new([
            reply(structured(serde_json::json!({
                "user_intent":["Fix the parser"],
                "user_messages":["Keep the v1 wire format"],
                "pending_tasks":["Add a regression test"],
                "current_work":"Parser fix modified, not yet tested",
                "next_step":"Run the parser tests",
                "files":[{"path":"src/parser.rs","summary":"Off-by-one fixed"}]
            }))),
            reply(structured(serde_json::json!({
                "user_intent":["Fix the parser"],
                "user_messages":["Keep the v1 wire format"],
                "pending_tasks":["Add a regression test"],
                "current_work":"Parser fix modified and tested; regression test in progress",
                "problem_solving":["Kept the scanner API because callers depend on it"],
                "next_step":"Commit the parser fix with its regression test"
            }))),
        ]);
        let model = model(provider.clone());
        let first = summarize(&model, history()).await.unwrap();
        for heading in ["## Goal", "## Constraints", "## State", "## Next Steps"] {
            assert!(first.contains(heading), "{first}");
        }
        assert!(first.contains("Open tasks:\n- Add a regression test"));
        assert_eq!(model.files.lock().unwrap()[0].path, "src/parser.rs");
        let second = summarize(
            &model,
            vec![
                Message::user().with_text(first.clone()),
                Message::user().with_text("The parser tests pass now"),
            ],
        )
        .await
        .unwrap();
        // The earlier handoff reaches the second request for merging.
        let system = provider.systems.lock().unwrap()[1].clone();
        assert!(system.contains("Keep the v1 wire format"));
        assert!(system.contains("Add a regression test"));
        for kept in [
            "- Fix the parser",
            "- Keep the v1 wire format",
            "- Add a regression test",
            "## Decisions",
            "Commit the parser fix with its regression test",
        ] {
            assert!(second.contains(kept), "{second}");
        }
        assert!(model.files.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancels_pending_request() {
        let directory = tempfile::tempdir().unwrap();
        let node = crate::Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let stop = CancellationToken::new();
        let invocation = invocation(&node);
        let provider = Arc::new(Pending::default());
        let operation = Compaction {
            provider: provider.clone(),
            config: ModelConfig::new("pending-fixture"),
            context: 64_000,
            threshold: 0.8,
            preparer: preparer(&node, &invocation, &stop),
            manual: true,
        };
        let session = super::super::super::tests::session(vec![(
            invocation.turn.id,
            Message::user()
                .with_generated_id()
                .with_text("Keep the original history"),
        )]);
        let (sender, _) = tokio::sync::mpsc::unbounded_channel();
        let emit = Emitter::new(sender, stop.clone());
        let machine = StateMachine::new(vec![Step::Operation(Arc::new(operation))], stop.clone());
        let cancel = {
            let provider = provider.clone();
            let stop = stop.clone();
            tokio::spawn(async move {
                provider.started.notified().await;
                stop.cancel();
            })
        };
        let result = machine.step(&session, &emit).await.unwrap();
        cancel.await.unwrap();
        assert_eq!(result.status, RunStatus::Cancelled);
        assert!(result.effects.is_empty());
        assert!(provider.dropped.load(Ordering::SeqCst));
        node.shutdown().await.unwrap();
    }
}
