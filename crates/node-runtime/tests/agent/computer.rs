//! Native Cua catalog and shared ADK routing, with no operating-system input.
#![cfg(unix)]
use super::*;
use serde_json::{Value, json};

#[path = "computer/catalog.rs"]
mod catalog;
#[path = "computer/continuity.rs"]
mod continuity;
#[path = "computer/dispatch.rs"]
mod dispatch;
#[path = "../support/computer.rs"]
mod driver;
#[cfg(target_os = "macos")]
#[path = "computer/manual.rs"]
mod manual;
#[path = "computer/media.rs"]
mod media;

async fn tool_result(fixture: &Fixture, revision: u64, name: &str) -> Value {
    let mut before = None;
    loop {
        let Output::TurnHistory(chunk) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadTurn {
                session: fixture.session,
                turn: fixture.turn,
                expected_revision: revision,
                before,
                limit: 100,
            }))
            .await
            .unwrap()
        else {
            panic!("turn history expected")
        };
        if let Some(result) = chunk
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .find_map(|part| match part {
                Part::ToolResult {
                    name: tool, result, ..
                } if tool == name => Some(result.clone()),
                _ => None,
            })
        {
            return result;
        }
        before = Some(chunk.next_before.expect("tool result missing from history"));
    }
}

pub(super) async fn set_enabled(client: &Client, enabled: bool) {
    let Output::Plugin(info) = client
        .execute(client.prepare(Command::ReadPlugin {
            name: "computer".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("computer package expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    assert!(info.extension.is_some());
    client
        .execute(client.prepare(Command::SetPluginEnabled {
            name: info.summary.name,
            expected_revision: info.summary.revision,
            enabled,
        }))
        .await
        .unwrap();
}

struct Fixture {
    _directory: tempfile::TempDir,
    node: Node,
    controller: Link,
    client: Client,
    session: SessionId,
    turn: TurnId,
    server: Server,
    worker: driver::Worker,
    record_start: usize,
    call_start: usize,
}

impl Fixture {
    async fn new(
        remote: bool,
        calls: Vec<(String, Value)>,
        mode: WorkMode,
        permission: Permission,
        enabled: bool,
        outcomes: Value,
    ) -> Self {
        Self::with_server(
            remote,
            Server::tools(calls).await,
            mode,
            permission,
            enabled,
            outcomes,
        )
        .await
    }

    async fn with_server(
        remote: bool,
        server: Server,
        mode: WorkMode,
        permission: Permission,
        enabled: bool,
        outcomes: Value,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let worker = driver::Worker::new(directory.path(), outcomes);
        let node = Node::start_empty_with_computer(
            directory.path().join("node"),
            NetworkScope::default(),
            Some(worker.configuration.clone()),
        )
        .await
        .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        super::approvals::install(&client, &["computer"]).await;
        let (session, turn) =
            Self::configure(&client, &server, &root, mode, permission, enabled).await;
        Self {
            _directory: directory,
            node,
            controller,
            client,
            session,
            turn,
            server,
            worker,
            record_start: 0,
            call_start: 0,
        }
    }

    async fn configure(
        client: &Client,
        server: &Server,
        root: &std::path::Path,
        mode: WorkMode,
        permission: Permission,
        enabled: bool,
    ) -> (SessionId, TurnId) {
        let (session, provider) = configured_session(client, &server.endpoint, root).await;
        let session = super::approvals::prepare_model(client, session, provider).await;
        let Output::Providers(providers) = client
            .execute(client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        let mut provider = providers
            .into_iter()
            .find(|provider| provider.id == session.config.provider)
            .unwrap();
        provider.models[0].vision = true;
        client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        let mut config = session.config;
        config.permission = permission;
        config.mode = mode;
        let Output::Session(session) = client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        set_enabled(client, enabled).await;
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Exercise native computer tools".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        (session.id, turn.id)
    }

    async fn next(
        mut self,
        server: Server,
        mode: WorkMode,
        permission: Permission,
        enabled: bool,
    ) -> Self {
        let Output::Session(previous) = self
            .client
            .execute(self.client.prepare(Command::ReadSession {
                session: self.session,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        self.client
            .execute(self.client.prepare(Command::RemoveSession {
                session: previous.id,
                expected_revision: previous.revision,
            }))
            .await
            .unwrap();
        // Observe each independent session after the previous owner's real cleanup.
        let records = self.worker.records();
        self.record_start = records.len();
        self.call_start = records
            .iter()
            .filter(|record| record["operation"] == "call" && record["name"] != "end_session")
            .count();
        let root = self._directory.path().join(SessionId::new().to_string());
        std::fs::create_dir(&root).unwrap();
        let (session, turn) =
            Self::configure(&self.client, &server, &root, mode, permission, enabled).await;
        self.session = session;
        self.turn = turn;
        self.server = server;
        self
    }

    fn records(&self) -> Vec<Value> {
        self.worker
            .records()
            .into_iter()
            .skip(self.record_start)
            .collect()
    }

    fn calls(&self) -> Vec<Value> {
        self.worker
            .calls()
            .into_iter()
            .skip(self.call_start)
            .collect()
    }

    async fn shutdown(self) {
        drop(self.client);
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}
