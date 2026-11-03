use super::*;
use crate::store::agent::Invocation;
use goose_agent::machine::{StateMachine, Step};
use goose_provider_types::{base::MessageStream, conversation::token_usage::Usage};
use rmcp::model::Tool;
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
}

#[derive(Default)]
struct Scripted {
    replies: Mutex<VecDeque<Reply>>,
    systems: Mutex<Vec<String>>,
}

impl Scripted {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into_iter().collect()),
            systems: Mutex::default(),
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
        _: &[Message],
        _: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        self.systems.lock().unwrap().push(system.into());
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
                ProviderUsage::new("scripted-fixture".into(), Usage::default())
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
    let mut output = Output::default();
    let messages = super::input::prepare(&messages, 64_000, 8_000)?;
    goose_context_management::compact(model, Some(&Estimator), &Input(messages), &mut output)
        .await?;
    Ok(output.summary.unwrap().as_concat_text())
}

fn invocation(node: &crate::Node) -> Arc<Invocation> {
    Arc::new(Invocation {
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

mod summary {
    use super::*;

    fn history() -> Vec<Message> {
        vec![Message::user().with_text("Fix the parser and keep the v1 wire format")]
    }

    #[tokio::test]
    async fn rejects_truncated_output() {
        let valid = structured(serde_json::json!({"user_intent":["Fix the parser"]}));
        for (limited, finish) in [(true, "stop"), (false, "length"), (false, "max_tokens")] {
            let provider = Scripted::new([Reply {
                text: valid.clone(),
                limited,
                finish,
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
        assert!(!system.contains("{{ messages }}"));
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
