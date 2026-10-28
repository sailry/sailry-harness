use super::*;
use crate::{
    agent::canonical::{Configuration, Message, Role},
    store::{Store, agent::SessionQuery},
};
use sailry_client::Client;
use sailry_link::{Identity, Local};
use sailry_protocol::{Command, Output, Permission, ProviderId, SessionConfig, WorkMode};

struct Fixture {
    _directory: tempfile::TempDir,
    _profile: crate::Profile,
    store: Store,
    client: Client,
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
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: None,
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
                message: "Original user text".into(),
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
            _directory: directory,
            _profile: profile,
            store,
            client,
            invocation,
        }
    }

    async fn history(&self) -> crate::store::agent::RuntimeSession {
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

    async fn context(&self) {
        prompt::session::Session::load(&self.store.ingress, &self.invocation)
            .await
            .unwrap()
            .append(
                &self.store.ingress,
                &self.invocation,
                "Changed policy".into(),
                "Environment fixture".into(),
                "Personal fixture".into(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
    }

    async fn append(&self, batch: &Batch) {
        batch
            .append(
                &self.store.ingress,
                self.invocation.turn.session,
                self.invocation.turn.id,
                "user",
                false,
            )
            .await
            .unwrap();
    }

    async fn close(self) {
        self.store.ingress.agents.finish(self.invocation.turn.id);
        self.store.stop_admission();
        self.store.shutdown().await.unwrap();
    }
}

fn entry(turn: TurnId, texts: &[&str]) -> Entry {
    let mut record = Record::new(
        turn,
        Kind::Message(Message {
            role: Role::User,
            part_count: texts.len(),
            content: texts
                .iter()
                .enumerate()
                .map(|(index, text)| Block {
                    index,
                    kind: BlockKind::Text {
                        text: (*text).into(),
                    },
                })
                .collect(),
            ..Default::default()
        }),
    );
    record.provenance.author = "user".into();
    Entry {
        turn,
        references: vec![],
        record,
        capability: None,
    }
}

#[tokio::test]
async fn writes_context_before_new_input() {
    let fixture = Fixture::new().await;
    let owner = fixture.invocation.turn.id;
    let batch = Batch {
        entries: vec![entry(owner, &["Original user text"])],
    };
    fixture.context().await;
    let context = fixture.history().await.records.remove(0);
    fixture.append(&batch).await;
    let history = fixture.history().await;
    let records = &history.records;
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].id, context.id);
    let Kind::Configuration(Configuration { context }) = &records[0].kind else {
        panic!("configuration expected")
    };
    assert_eq!(context.policy.as_deref(), Some("Changed policy"));
    assert_eq!(
        records[1].metadata.input.as_ref().unwrap().turn,
        Some(owner)
    );
    assert!(matches!(
        &records[1].message().unwrap().content[0].kind,
        BlockKind::Text { text } if text == "Original user text"
    ));
    fixture.close().await;
}

#[tokio::test]
async fn appends_changed_context_after_existing_input() {
    let fixture = Fixture::new().await;
    let owner = fixture.invocation.turn.id;
    let mut input = entry(owner, &["Selected references", "Original user text"]);
    input.references.push(Reference {
        target: sailry_protocol::conversation::reference::Target::Plugin("fixture".into()),
        label: "Fixture".into(),
    });
    let batch = Batch {
        entries: vec![input],
    };
    fixture.append(&batch).await;
    fixture.context().await;
    let first = fixture.history().await;
    let context = first.records[1].id.clone();
    fixture.context().await;
    fixture.append(&batch).await;
    let history = fixture.history().await;
    let records = &history.records;
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].id, context);
    assert_eq!(
        records[1].context().unwrap().policy.as_deref(),
        Some("Changed policy")
    );
    let input = records[0].metadata.input.as_ref().unwrap();
    assert_eq!(input.capability_part, None);
    assert_eq!(input.text_part, Some(1));
    let Output::Conversation(history) = fixture
        .client
        .execute(fixture.client.prepare(Command::ReadConversation {
            session: fixture.invocation.turn.session,
            before: None,
            limit: 20,
        }))
        .await
        .unwrap()
    else {
        panic!("conversation history expected")
    };
    assert!(history.page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part, sailry_protocol::conversation::Part::Text(text) if text == "Original user text")));
    fixture.close().await;
}

#[tokio::test]
async fn resumes_partially_saved_inputs() {
    let fixture = Fixture::new().await;
    let owner = fixture.invocation.turn.id;
    let mut batch = Batch {
        entries: vec![entry(owner, &["Original user text"])],
    };
    fixture.context().await;
    let original_context = fixture.history().await.records[0].id.clone();
    fixture.append(&batch).await;
    let Output::QueuedTurn(next) = fixture
        .client
        .execute(fixture.client.prepare(Command::QueueTurn {
            session: fixture.invocation.turn.session,
            expected_revision: 1,
            message: "Remaining input".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("queued input expected")
    };
    batch.entries.push(entry(next.id, &["Remaining input"]));
    fixture.context().await;
    fixture.append(&batch).await;
    let history = fixture.history().await;
    let records = &history.records;
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].id, original_context);
    assert_eq!(
        records[1].metadata.input.as_ref().unwrap().turn,
        Some(owner)
    );
    assert_eq!(
        records[2].metadata.input.as_ref().unwrap().turn,
        Some(next.id)
    );
    assert!(matches!(
        &records[2].message().unwrap().content[0].kind,
        BlockKind::Text { text } if text == "Remaining input"
    ));
    fixture.close().await;
}

#[tokio::test]
async fn stamps_exact_capability_position() {
    let fixture = Fixture::new().await;
    let mut input = entry(
        fixture.invocation.turn.id,
        &[
            "Original user text",
            "Selected capability",
            "Following user data",
        ],
    );
    input.capability = Some(1);
    fixture
        .append(&Batch {
            entries: vec![input],
        })
        .await;
    let history = fixture.history().await;
    assert_eq!(
        history.records[0]
            .metadata
            .input
            .as_ref()
            .unwrap()
            .capability_part,
        Some(1)
    );
    let Output::Conversation(history) = fixture
        .client
        .execute(fixture.client.prepare(Command::ReadConversation {
            session: fixture.invocation.turn.session,
            before: None,
            limit: 20,
        }))
        .await
        .unwrap()
    else {
        panic!("conversation history expected")
    };
    let parts: Vec<_> = history
        .page
        .entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .collect();
    assert!(parts.iter().any(|part| matches!(part, sailry_protocol::conversation::Part::Text(text) if text == "Original user text")));
    assert!(parts.iter().any(|part| matches!(part, sailry_protocol::conversation::Part::Text(text) if text == "Following user data")));
    assert!(!parts.iter().any(|part| matches!(part, sailry_protocol::conversation::Part::Text(text) if text == "Selected capability")));
    fixture.close().await;
}
