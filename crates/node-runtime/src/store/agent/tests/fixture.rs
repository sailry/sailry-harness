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
                    config: Some(configuration()),
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

    pub async fn fresh_session(&mut self) {
        // Retain earlier scenario history while keeping its pending work out of later claims.
        let mut statement = self
            .database
            .connection
            .prepare("SELECT id FROM sessions")
            .await
            .unwrap();
        let mut rows = statement.query(()).await.unwrap();
        let mut previous = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            previous.push(row.get::<String>(0).unwrap().parse::<SessionId>().unwrap());
        }
        drop(rows);
        drop(statement);
        for session in previous {
            queue::pause(&self.database.connection, session, true)
                .await
                .unwrap();
        }
        let path = self._root.path().join(ProjectId::new().to_string());
        std::fs::create_dir(&path).unwrap();
        let Output::Project(project) = command(
            &mut self.database,
            &self.events,
            Command::RegisterProject {
                name: "Turn history fixture".into(),
                path: path.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let Output::Session(session) = command(
            &mut self.database,
            &self.events,
            Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(configuration()),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        self.session = session.id;
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
            message(
                "assistant",
                crate::agent::canonical::Role::Assistant,
                vec![crate::agent::canonical::BlockKind::Text {
                    text: (*content).into(),
                }],
            )
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

    pub fn append(
        &mut self,
        events: impl IntoIterator<Item = crate::agent::canonical::Record>,
    ) -> sailry_link::Pending<'_, TurnId> {
        let events: Vec<_> = events.into_iter().collect();
        Box::pin(async move {
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
            for mut event in events {
                event.turn = turn;
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
        })
    }

    pub async fn read(&self, before: Option<TurnId>, limit: u16) -> History {
        super::super::history::pages::read(&self.database.connection, self.session, before, limit)
            .await
            .unwrap()
    }
}

fn configuration() -> SessionConfig {
    SessionConfig {
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

/// Third-party catalog editors keep using native role operations after delegation moved into core.
pub(in crate::store::agent) const ROLES: &str = "roles-fixture";

pub(in crate::store::agent) async fn roles(database: &Database) {
    let tool = |name: &str, operation: &str| {
        serde_json::json!({"name":name,"server":null,"operation":operation,"handler":null,
            "description":"Role fixture","display":null,"contexts":["workspace"]})
    };
    let info: sailry_protocol::plugin::Info = serde_json::from_value(serde_json::json!({
        "summary":{"name":ROLES,"revision":1,"settings_revision":0,"enabled":true,"digest":"b".repeat(64),"version":"0.1.0"},
        "extension":{"api_version":"v1","scope":"host","actions":["roles.read","roles.write"],
            "tools":[tool("edit_agent","roles.save"),tool("delete_agent","roles.remove"),tool("apply_agents","roles.apply")]}
    }))
    .unwrap();
    crate::store::plugins::install_fixture(&database.connection, &info)
        .await
        .unwrap();
    database
        .connection
        .execute(
            "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
            params![
                info.summary.digest.as_str(),
                info.summary.name.as_str(),
                crate::store::database::encode(&info).unwrap()
            ],
        )
        .await
        .unwrap();
}
