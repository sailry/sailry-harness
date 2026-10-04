use super::*;
use serde_json::{Value, json};

pub(super) fn model(id: &str) -> Model {
    Model {
        id: id.into(),
        context: 16000,
        output: 2048,
        vision: false,
        tools: true,
        reasoning: true,
        web_search: false,
        generates: vec![],
        efforts: vec![sailry_protocol::Effort::Low, sailry_protocol::Effort::High],
        custom_efforts: false,
        default_effort: sailry_protocol::Effort::Low,
    }
}

pub(super) async fn setup(
    client: &Client,
    root: &std::path::Path,
    parent: &Server,
    child: &Server,
) -> (Session, Provider, role::Profile) {
    let (session, mut provider) = configured(client, &parent.endpoint, root).await;
    provider.models = vec![model("fixture-a")];
    provider.default_model = "fixture-a".into();
    client
        .execute(client.prepare(Command::PutProvider {
            provider: provider.clone(),
            expected_revision: 1,
        }))
        .await
        .unwrap();
    provider.id = ProviderId::new();
    provider.endpoint = child.endpoint.clone();
    provider.revision = 0;
    provider.models = vec![model("fixture-child")];
    provider.default_model = "fixture-child".into();
    let Output::Provider(provider) = client
        .execute(client.prepare(Command::PutProvider {
            provider,
            expected_revision: 0,
        }))
        .await
        .unwrap()
    else {
        panic!("provider expected")
    };
    let role = role::Profile {
        appearance: None,
        id: RoleId::new(),
        revision: 0,
        key: "review".into(),
        name: "Review".into(),
        description: "Inspect the delegated task".into(),
        model: Some(role::Model {
            provider: provider.id,
            model: "fixture-child".into(),
            effort: None,
        }),
        max_turns: Some(5),
        skills: vec![],
        instructions: "Frozen review instructions".into(),
    };
    let Output::Role(role) = client
        .execute(client.prepare(Command::PutRole {
            role,
            expected_revision: 0,
        }))
        .await
        .unwrap()
    else {
        panic!("role expected")
    };
    let Output::Session(session) = client
        .execute(client.prepare(Command::SetSessionRoles {
            session: session.id,
            expected_revision: 1,
            roles: vec![role.reference()],
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    (session, provider, role)
}

fn results(page: &Page) -> Vec<Value> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::ToolResult { result, .. } => Some(result.clone()),
            _ => None,
        })
        .collect()
}

async fn children(client: &Client, parent: SessionId, count: usize) -> Vec<Session> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            let children: Vec<_> = snapshot
                .sessions
                .into_iter()
                .filter(|session| {
                    session
                        .delegation
                        .as_ref()
                        .is_some_and(|origin| origin.session == parent)
                })
                .collect();
            if children.len() == count {
                return children;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("child admission deadline")
}

mod approvals;
mod lifecycle;
mod parallel;
mod removal;
mod roles;
mod selection;
mod streaming;
