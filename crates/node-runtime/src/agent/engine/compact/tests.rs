use super::*;
use crate::store::agent::Invocation;
use goose_agent::machine::{StateMachine, Step};
use goose_provider_types::base::MessageStream;
use rmcp::model::Tool;
use sailry_link::CancellationToken;
use sailry_protocol::{conversation::RunKind, *};
use std::sync::atomic::{AtomicUsize, Ordering};

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

#[tokio::test]
async fn yields_managed_context_after_previous_error() {
    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let stop = CancellationToken::new();
    let invocation = Arc::new(Invocation {
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
    });
    let preparer = Arc::new(super::super::prompt::Preparer {
        ingress: node.ingress.clone(),
        invocation: invocation.clone(),
        stop: stop.clone(),
        deferred: Default::default(),
        images: vec![],
        sources: Default::default(),
    });
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
            context: 100,
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
