use super::*;
use crate::{
    agent::{canonical, engine},
    store::{Store, agent::RuntimeSession},
};
use sailry_client::Client;
use sailry_link::{Identity, Local};
use sailry_protocol::{Command, Output, Permission, ProviderId, SessionConfig, WorkMode};

struct Fixture {
    directory: tempfile::TempDir,
    _profile: crate::Profile,
    store: Store,
    invocation: Invocation,
}

impl Fixture {
    async fn new() -> Self {
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
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let Output::Project(project) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Prompt fixture".into(),
                path: root.to_str().unwrap().into(),
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
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Prompt fixture".into(),
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
        assert_eq!(invocation.turn.id, turn.id);
        Self {
            directory,
            _profile: profile,
            store,
            invocation,
        }
    }

    async fn append(&self, mode: WorkMode) {
        Session::load(&self.store.ingress, &self.invocation)
            .await
            .unwrap()
            .append(
                &self.store.ingress,
                &self.invocation,
                super::super::policy(mode, Permission::Ask, false).into(),
                "Environment fixture".into(),
                "Personal fixture".into(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
    }

    async fn history(&self) -> RuntimeSession {
        self.store
            .ingress
            .sessions(self.invocation.turn.id)
            .get(SessionQuery {
                session: self.invocation.turn.session,
                recent: None,
                after: None,
            })
            .await
            .unwrap()
    }

    async fn close(self) {
        self.store.ingress.agents.finish(self.invocation.turn.id);
        self.store.stop_admission();
        self.store.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn appends_only_changed_runtime_context() {
    let fixture = Fixture::new().await;
    fixture.append(WorkMode::Code).await;
    let first = fixture.history().await;
    assert_eq!(first.records.len(), 1);
    assert!(first.records[0].context().unwrap().captured);
    fixture.append(WorkMode::Code).await;
    assert_eq!(fixture.history().await.records.len(), 1);
    fixture.append(WorkMode::Plan).await;
    let changed = fixture.history().await;
    assert_eq!(changed.records.len(), 2);
    let delta = changed.records[1].context().unwrap();
    assert!(
        delta
            .policy
            .as_ref()
            .unwrap()
            .contains("This is a planning turn")
    );
    assert!(delta.environment.is_none());
    assert!(delta.personal.is_none());
    assert!(delta.guides.is_empty());
    assert!(!changed.state.contains_key("sailry.context.declarations"));
    let request = engine::prompt::prepared(&changed).await;
    let prompt = request.system_prompt;
    assert!(prompt.contains("This is a planning turn"));
    assert!(!prompt.contains("Environment fixture"));
    assert_eq!(prompt.matches("Personal fixture").count(), 1);
    let [context] = request.additional_messages.as_slice() else {
        panic!("one turn context expected")
    };
    assert_eq!(
        context
            .as_concat_text()
            .matches("Environment fixture")
            .count(),
        1
    );
    fixture.close().await;
}

#[tokio::test]
async fn preserves_loaded_tools() {
    let fixture = Fixture::new().await;
    let loaded: BTreeSet<_> = ["fixture".into(), "removed".into()].into();
    let mut record = Record::new(
        fixture.invocation.turn.id,
        Kind::Configuration(canonical::Configuration {
            context: Context {
                loaded_tools: Some(loaded.clone()),
                ..Default::default()
            },
        }),
    );
    record.provenance.author = "system".into();
    fixture
        .store
        .ingress
        .sessions(fixture.invocation.turn.id)
        .append_event(&fixture.invocation.turn.session.to_string(), record)
        .await
        .unwrap();
    fixture.append(WorkMode::Code).await;
    let restored = Session::load(&fixture.store.ingress, &fixture.invocation)
        .await
        .unwrap();
    assert_eq!(restored.loaded(), loaded);
    let history = fixture.history().await;
    assert_eq!(history.context.loaded_tools.as_ref(), Some(&loaded));
    assert!(history.records[1].context().unwrap().loaded_tools.is_none());
    assert!(!history.state.contains_key("sailry.context.declarations"));
    fixture.close().await;
}

#[tokio::test]
async fn preserves_capability_sources() {
    let fixture = Fixture::new().await;
    for selected in ["Earlier selected skill", "Latest selected skill"] {
        let mut record = Record::new(
            fixture.invocation.turn.id,
            Kind::Message(canonical::Message {
                role: canonical::Role::User,
                part_count: 2,
                content: vec![
                    canonical::Block {
                        index: 0,
                        kind: canonical::BlockKind::Text {
                            text: "User task".into(),
                        },
                    },
                    canonical::Block {
                        index: 1,
                        kind: canonical::BlockKind::Text {
                            text: selected.into(),
                        },
                    },
                ],
                ..Default::default()
            }),
        );
        record.provenance.author = "user".into();
        record.metadata.input = Some(canonical::Input {
            turn: Some(fixture.invocation.turn.id),
            source: Some(canonical::InputSource::User),
            text_part: Some(0),
            capability_part: Some(1),
            ..Default::default()
        });
        fixture
            .store
            .ingress
            .sessions(fixture.invocation.turn.id)
            .append_event(&fixture.invocation.turn.session.to_string(), record)
            .await
            .unwrap();
    }
    let history = fixture.history().await;
    let records = serde_json::to_string(&history.records).unwrap();
    assert!(records.contains("Latest selected skill"));
    assert!(records.contains("Earlier selected skill"));
    let prompt = engine::prompt::prepared(&history).await.system_prompt;
    assert_eq!(prompt.matches("Latest selected skill").count(), 1);
    assert!(!prompt.contains("Earlier selected skill"));
    fixture.close().await;
}

#[tokio::test]
async fn materializes_exact_captured_context() {
    let fixture = Fixture::new().await;
    let guide = fixture.directory.path().join("project/AGENTS.md");
    std::fs::write(&guide, "Old guide content").unwrap();
    fixture.append(WorkMode::Code).await;
    std::fs::write(&guide, "Current guide content").unwrap();
    fixture.append(WorkMode::Plan).await;
    let captured = fixture.history().await;
    let first = engine::prompt::prepared(&captured).await.system_prompt;
    assert_eq!(first.matches("This is a planning turn").count(), 1);
    assert_eq!(first.matches("Current guide content").count(), 1);
    assert!(!first.contains("Old guide content"));
    assert_eq!(captured.context.guides.len(), 1);
    assert_eq!(captured.records.len(), 2);
    assert!(
        captured.records[0]
            .context()
            .unwrap()
            .guides
            .values()
            .any(|guide| guide.content.contains("Old guide content"))
    );
    assert!(!captured.state.contains_key("sailry.context.declarations"));
    let restored = fixture.history().await;
    let second = engine::prompt::prepared(&restored).await.system_prompt;
    assert_eq!(first, second);
    assert_eq!(second.matches("Current guide content").count(), 1);
    fixture.close().await;
}
