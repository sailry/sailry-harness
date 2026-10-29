use super::*;
use crate::{
    agent::{
        canonical::{Block, BlockKind, Kind, Message, Record, ToolRequest},
        catalog::{Catalog, Registration},
        engine::Children,
    },
    store::{Store, agent::SessionQuery},
    tasks::Supervisor,
};
use goose_agent::{operation::Emitter, tool::ToolProvider};
use sailry_client::Client;
use sailry_link::{Identity, Local};
use sailry_protocol::{Command, Output, Permission, ProviderId, SessionConfig, WorkMode};
use std::sync::Mutex;
use tokio::sync::{Notify, oneshot};

pub(super) struct Handoff {
    entered: Mutex<Option<oneshot::Sender<(SessionId, TurnId)>>>,
    resume: Notify,
}

impl Handoff {
    pub(super) async fn wait(&self, session: SessionId, turn: TurnId) {
        self.entered
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .send((session, turn))
            .unwrap();
        self.resume.notified().await;
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    _profile: crate::Profile,
    store: Store,
    invocation: Invocation,
    call: rmcp::model::CallToolRequestParams,
}

impl Fixture {
    async fn new() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let directory = tempfile::tempdir().unwrap();
        let profile = crate::Profile::acquire(&directory.path().join("node")).unwrap();
        let identity = Identity::open(&profile.path).unwrap();
        let store = Store::open(
            profile.path.clone(),
            identity.id(),
            crate::providers::catalog::Catalog::empty(),
            Default::default(),
            Default::default(),
            Default::default(),
            false,
        )
        .await
        .unwrap();
        let client = Client::new(Arc::new(Local::new(
            identity.id(),
            identity.id(),
            store.ingress.clone(),
        )));
        let Output::Plugin(package) = client
            .execute(client.prepare(Command::InstallBundledPlugin {
                name: "delegation".into(),
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("plugin expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let Output::Project(project) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Handoff fixture".into(),
                path: directory.path().to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: Default::default(),
                    features: Default::default(),
                    mode: WorkMode::Code,
                    options: Default::default(),
                    permission: Permission::Ask,
                    credential: None,
                }),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(queued) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Delegate once".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let invocation = store
            .ingress
            .claim_run(CancellationToken::new())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(invocation.turn.id, queued.id);
        let name = crate::plugins::tools::alias("delegation", "spawn_agent");
        let arguments = json!({"task":"Read the frozen context"});
        let mut record = Record::new(
            queued.id,
            Kind::Message(Message {
                part_count: 1,
                content: vec![Block {
                    index: 0,
                    kind: BlockKind::ToolRequest(ToolRequest {
                        id: "call-child".into(),
                        name: name.clone(),
                        arguments: arguments.clone(),
                        provider: None,
                        tool_meta: None,
                        error: None,
                    }),
                }],
                ..Default::default()
            }),
        );
        record.provenance.author = "assistant".into();
        store
            .ingress
            .sessions(queued.id)
            .append_event(&session.id.to_string(), record)
            .await
            .unwrap();
        let call = rmcp::model::CallToolRequestParams::new(name)
            .with_arguments(arguments.as_object().unwrap().clone());
        Self {
            _directory: directory,
            _profile: profile,
            store,
            invocation,
            call,
        }
    }
}

#[tokio::test]
async fn completes_handoff_after_caller_drop() {
    let fixture = Box::pin(Fixture::new()).await;
    let supervisor = Supervisor::start(4);
    let ingress = &fixture.store.ingress;
    let turn = fixture.invocation.turn.id;
    let children = Children::new(
        ingress.clone(),
        supervisor.tasks.clone(),
        &fixture.invocation,
    );
    let stop = CancellationToken::new();
    let (entered, mut admitted) = oneshot::channel();
    let handoff = Arc::new(Handoff {
        entered: Mutex::new(Some(entered)),
        resume: Notify::new(),
    });
    let mut tool = Delegate::new(
        ingress,
        &supervisor.tasks,
        &fixture.invocation,
        &stop,
        fixture.call.name.to_string(),
        "Delegate fixture".into(),
        json!({"type":"object"}),
    );
    tool.handoff = Some(handoff.clone());
    let mut catalog = Catalog::new(fixture.invocation.turn.session.to_string(), stop, None)
        .with_children(children.clone());
    catalog
        .register(Registration::new(
            tool.descriptor(),
            Arc::new(tool),
            Default::default(),
        ))
        .unwrap();
    let session = ingress
        .sessions(turn)
        .get(SessionQuery {
            session: fixture.invocation.turn.session,
            recent: None,
            after: None,
        })
        .await
        .unwrap();
    let (events, _updates) = tokio::sync::mpsc::unbounded_channel();
    let emit = Emitter::new(events, CancellationToken::new());
    let mut caller = Box::pin(catalog.call(&session, "call-child", fixture.call.clone(), &emit));
    let (child, child_turn) = tokio::select! {
        result = &mut caller => panic!("handoff ended before cancellation: {result:?}"),
        child = &mut admitted => child.unwrap(),
    };
    assert_eq!(
        ingress.child_state(turn, child).await.unwrap().run.status,
        Status::Queued
    );
    assert_eq!(ingress.agents.operations(turn), 1);

    // The real actor has committed the child, while the caller has no receipt yet.
    drop(caller);
    ingress.agents.cancel(turn);
    let mut drained = Box::pin(ingress.agents.drain(turn));
    assert!(futures::poll!(&mut drained).is_pending());
    handoff.resume.notify_one();
    tokio::time::timeout(Duration::from_secs(5), drained)
        .await
        .unwrap();
    assert_eq!(ingress.agents.operations(turn), 0);
    children.settle_pending().await.unwrap();
    let state = ingress.child_state(turn, child).await.unwrap();
    assert_eq!(state.run.turn, child_turn);
    assert_eq!(state.run.status, Status::Cancelled);
    assert!(
        ingress
            .claim_run(CancellationToken::new())
            .await
            .unwrap()
            .is_none()
    );
    assert!(ingress.agents.begin_operation(child_turn).is_none());

    ingress
        .finish_run(turn, Status::Cancelled, None, None)
        .await
        .unwrap();
    ingress.agents.finish(turn);
    supervisor.shutdown(Duration::from_secs(5)).await.unwrap();
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn ignores_previous_turn_child_requests() {
    use goose_agent::subagent::SubagentRunner;
    use goose_provider_types::conversation::message::Message as GdkMessage;
    let fixture = Box::pin(Fixture::new()).await;
    let supervisor = Supervisor::start(1);
    let ingress = &fixture.store.ingress;
    let turn = fixture.invocation.turn.id;
    let children = Children::new(
        ingress.clone(),
        supervisor.tasks.clone(),
        &fixture.invocation,
    );
    let mut session = ingress
        .sessions(turn)
        .get(SessionQuery {
            session: fixture.invocation.turn.session,
            recent: None,
            after: None,
        })
        .await
        .unwrap();
    let mut result = rmcp::model::CallToolResult::structured(json!({"session_id":"child"}));
    result.meta = Some(
        serde_json::from_value(json!({"foreground_subagent":true,"subagent_session_id":"child"}))
            .unwrap(),
    );
    let message = GdkMessage::user()
        .with_generated_id()
        .with_tool_response("call-child", Ok(result));
    let mut record = Record::new(turn, Kind::Message(Message::default()));
    record.id = message.id.clone().unwrap();
    session.records.push(record);
    assert_eq!(
        children.requested_subagent_session_ids(&session, &message),
        vec!["child"]
    );
    session.records.last_mut().unwrap().turn = TurnId::new();
    assert!(
        children
            .requested_subagent_session_ids(&session, &message)
            .is_empty()
    );
    supervisor.shutdown(Duration::from_secs(5)).await.unwrap();
    fixture.store.shutdown().await.unwrap();
}
