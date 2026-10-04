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
        let node = Node::start_with_computer(
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
        let session = super::approvals::prepare(&client, &server, &root).await;
        let Output::Providers(mut providers) = client
            .execute(client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        let mut provider = providers.remove(0);
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
        set_enabled(&client, enabled).await;
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
        Self {
            _directory: directory,
            node,
            controller,
            client,
            session: session.id,
            turn: turn.id,
            server,
            worker,
        }
    }

    async fn shutdown(self) {
        drop(self.client);
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}
