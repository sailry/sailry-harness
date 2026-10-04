use super::configuration::{config, provider};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output, SessionId};
use std::path::Path;

pub(super) async fn sessions(
    directory: &Path,
    endpoint: &str,
    model: &str,
) -> (Node, Vec<SessionId>) {
    let node = Node::start(directory.join("node")).await.unwrap();
    let client = Client::new(node.local());
    for name in ["context7", "github", "web-search", "goals", "office"] {
        let Output::Plugin(package) = client
            .execute(client.prepare(Command::ReadPlugin { name: name.into() }))
            .await
            .unwrap()
        else {
            panic!("package expected")
        };
        if package.summary.enabled {
            client
                .execute(client.prepare(Command::SetPluginEnabled {
                    name: package.summary.name,
                    expected_revision: package.summary.revision,
                    enabled: false,
                }))
                .await
                .unwrap();
        }
    }
    let mut selected = provider(&client, endpoint, model).await;
    selected.models[0].tools = true;
    client
        .execute(client.prepare(Command::PutProvider {
            provider: selected.clone(),
            expected_revision: selected.revision,
        }))
        .await
        .unwrap();
    let Output::Project(project) = client
        .execute(client.prepare(Command::RegisterProject {
            name: "FFI tools".into(),
            path: directory.join("project").to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let mut sessions = Vec::new();
    for _ in 0..2 {
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(config(&selected)),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        sessions.push(session.id);
    }
    (node, sessions)
}

pub(super) async fn pages(
    client: &Client,
    sessions: &[SessionId],
) -> Vec<sailry_protocol::conversation::Page> {
    let mut pages = Vec::new();
    for session in sessions {
        pages.push(
            client
                .read_conversation(*session, None, 100)
                .await
                .unwrap()
                .page,
        );
    }
    pages
}
