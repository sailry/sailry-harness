use super::super::*;
use super::command;

mod records;

pub(in crate::store::agent) use records::{message, record, request, response};

pub(in crate::store::agent) struct Fixture {
    pub database: Database,
    pub events: broadcast::Sender<EventEnvelope>,
    pub session: SessionId,
    pub _root: tempfile::TempDir,
}

impl Fixture {
    pub fn new() -> sailry_link::Pending<'static, Self> {
        Box::pin(async move {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().canonicalize().unwrap().join("node.turso");
            let mut database = Database::open(&path, NodeId([33; 32]), None).await.unwrap();
            let (events, _) = broadcast::channel(64);
            let Output::Project(project) = command(
                &mut database,
                &events,
                Command::RegisterProject {
                    name: "Turn history fixture".into(),
                    path: root.path().to_str().unwrap().into(),
                },
            )
            .await
            else {
                panic!("project expected")
            };
            let Output::Session(session) = command(
                &mut database,
                &events,
                Command::CreateSession {
                    project: Some(project.id),
                    worktree: None,
                    config: Some(SessionConfig {
                        features: Default::default(),
                        assistant: None,
                        resource: None,
                        provider: ProviderId::new(),
                        model: "fixture".into(),
                        effort: Effort::Low,
                        mode: sailry_protocol::WorkMode::Code,
                        options: Default::default(),
                        permission: Permission::Ask,
                        credential: None,
                    }),
                },
            )
            .await
            else {
                panic!("session expected")
            };
            Self {
                database,
                events,
                session: session.id,
                _root: root,
            }
        })
    }

    pub async fn queued(&mut self) -> TurnId {
        let revision = crate::store::commands::session(&self.database.connection, self.session)
            .await
            .unwrap()
            .revision;
        let Output::QueuedTurn(turn) = command(
            &mut self.database,
            &self.events,
            Command::QueueTurn {
                session: self.session,
                expected_revision: revision,
                message: "History fixture".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        turn.id
    }

    pub async fn completed(&mut self, contents: &[&str]) -> TurnId {
        self.append(contents.iter().map(|content| {
            let mut event = AdkEvent::new("history-fixture");
            event.author = "assistant".into();
            event.set_content(adk_core::Content::new("model").with_text(*content));
            event
        }))
        .await
    }

    pub async fn fork(&mut self, source: SessionId, through: TurnId) -> Session {
        let revision = crate::store::commands::session(&self.database.connection, source)
            .await
            .unwrap()
            .revision;
        let Output::Session(session) = command(
            &mut self.database,
            &self.events,
            Command::ForkConversation {
                session: source,
                through,
                expected_revision: revision,
            },
        )
        .await
        else {
            panic!("fork expected")
        };
        session
    }

    pub async fn append(&mut self, events: impl IntoIterator<Item = AdkEvent>) -> TurnId {
        let turn = self.queued().await;
        runs::start(&self.database.connection, turn).await.unwrap();
        assert_eq!(
            runs::claim(&mut self.database, &self.events)
                .await
                .unwrap()
                .unwrap()
                .turn
                .id,
            turn
        );
        for event in events {
            super::super::history::append(
                &mut self.database,
                self.session,
                turn,
                event,
                &self.events,
            )
            .await
            .unwrap();
        }
        runs::finish(
            &mut self.database,
            turn,
            Status::Completed,
            None,
            &self.events,
        )
        .await
        .unwrap();
        turn
    }

    pub async fn read(&self, before: Option<TurnId>, limit: u16) -> History {
        super::super::history::pages::read(&self.database.connection, self.session, before, limit)
            .await
            .unwrap()
    }
}

/// Install the actual ordinary provider before capturing an approval fixture's turn.
pub(in crate::store::agent) async fn files(database: &Database, profile: &std::path::Path) {
    package(database, profile, "files").await;
}

pub(in crate::store::agent) async fn package(
    database: &Database,
    profile: &std::path::Path,
    name: &str,
) {
    if crate::store::plugins::get(&database.connection, name)
        .await
        .unwrap()
        .is_some()
    {
        return;
    }
    let package = crate::plugins::Host::new(Some(profile.canonicalize().unwrap()))
        .install_bundled(name)
        .unwrap();
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    let mut result = Ok(Output::Plugin(package));
    let transaction = database.connection.unchecked_transaction().await.unwrap();
    crate::store::plugins::finish(
        &transaction,
        Some(profile),
        &Command::InstallBundledPlugin {
            name: name.into(),
            expected_revision: 0,
        },
        &mut result,
    )
    .await
    .unwrap();
    result.unwrap();
    transaction.commit().await.unwrap();
}
