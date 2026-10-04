use super::*;
use process::Fixture;
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
#[path = "goals/execution.rs"]
mod execution;
#[path = "goals/packages.rs"]
mod packages;

async fn fixture(remote: bool, server: &Server) -> Fixture {
    let fixture = Fixture::new(remote, server).await;
    fixture
        .client
        .execute(fixture.client.prepare(Command::SetPluginEnabled {
            name: "goals".into(),
            expected_revision: 2,
            enabled: true,
        }))
        .await
        .unwrap();
    fixture
}

async fn call(fixture: &Fixture, name: &str, handler: &str, input: Value) -> Value {
    let Output::Plugin(package) = fixture
        .client
        .execute(
            fixture
                .client
                .prepare(Command::ReadPlugin { name: name.into() }),
        )
        .await
        .unwrap()
    else {
        panic!("package expected")
    };
    let request = fixture
        .client
        .prepare(Command::CallPlugin {
            handler: handler.into(),
            input,
        })
        .with_plugin(plugin::Context {
            invocation: None,
            turn: None,
            surface: plugin::desktop::Surface::Workspace,
            package: package.summary.reference(),
            worktree: Some(fixture.session.worktree),
            session: Some(fixture.session.id),
        });
    let Output::PluginResult(value) = fixture.client.execute(request).await.unwrap() else {
        panic!("plugin result expected")
    };
    value
}

async fn read(fixture: &Fixture) -> Value {
    call(fixture, "goals", "read", json!({})).await["goal"].clone()
}

async fn observe(
    client: &Client,
    session: SessionId,
) -> Arc<sailry_protocol::conversation::Snapshot> {
    let (sender, mut receiver) = watch::channel(sailry_client::conversation::View::default());
    let (_history, requests) = mpsc::channel(1);
    let stop = CancellationToken::new();
    let observer = client.watch_conversation(session, sender, stop.clone(), requests);
    let read = async {
        let snapshot = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                receiver.changed().await.unwrap();
                let view = receiver.borrow_and_update().clone();
                if view.connected
                    && let Some(snapshot) = view.snapshot
                {
                    break snapshot;
                }
            }
        })
        .await
        .unwrap();
        stop.cancel();
        snapshot
    };
    let (result, snapshot) = tokio::join!(observer, read);
    result.unwrap();
    snapshot
}

fn results(page: &Page) -> Vec<(&str, &Value)> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::ToolResult { name, result, .. } => Some((name.as_str(), result)),
            _ => None,
        })
        .collect()
}
