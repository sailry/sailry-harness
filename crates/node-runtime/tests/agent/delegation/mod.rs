use super::*;
use serde_json::{Value, json};

mod capacity;
mod catalog;
mod explore;
mod research;
mod unassigned;

pub(super) fn model(id: &str) -> Model {
    Model {
        id: id.into(),
        display_name: None,
        context: 128000,
        output: 2048,
        vision: false,
        tools: true,
        reasoning: true,
        web_search: false,
        generates: vec![],
        efforts: vec![sailry_protocol::Effort::Low, sailry_protocol::Effort::High],
        capabilities: None,
        custom_efforts: false,
        compaction_tokens: None,
        default_effort: sailry_protocol::Effort::Low,
    }
}

async fn install(client: &Client) {
    super::approvals::install(
        client,
        &["delegation", "files", "commands", "git", "worktrees"],
    )
    .await;
}

pub(super) async fn setup(
    client: &Client,
    root: &std::path::Path,
    parent: &Server,
    child: &Server,
) -> (Session, Provider, role::Profile) {
    install(client).await;
    let (session, mut provider) = configured_session(client, &parent.endpoint, root).await;
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
        read_only: false,
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
        tools: None,
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

fn prompt(request: &Value) -> String {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| matches!(message["role"].as_str(), Some("system" | "developer")))
        .map(|message| message_text(&message["content"]))
        .collect::<Vec<_>>()
        .join("\n")
}

async fn admissions(client: &Client, page: &Page) -> Vec<Value> {
    results(client, page)
        .await
        .into_iter()
        .filter(|value| value["admitted"] == true)
        .collect()
}

fn received(parent: &Server, session: SessionId, outcome: &str, count: usize) {
    let requests = parent.requests.lock().unwrap();
    let request = requests.last().expect("parent continuation expected");
    let outcome = format!("Subagent {session} {outcome}");
    let delivered = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .map(|message| message_text(&message["content"]).matches(&outcome).count())
        .sum::<usize>();
    assert_eq!(delivered, count, "each child outcome is delivered once");
}

async fn completed(client: &Client, page: &Page, parent: &Server, receipt: &Value, answer: &str) {
    assert_eq!(receipt["status"], "queued", "{receipt}");
    assert_eq!(receipt["admitted"], true);
    assert_eq!(receipt["pending"], true);
    let session: SessionId = serde_json::from_value(receipt["session"].clone()).unwrap();
    let turn: TurnId = serde_json::from_value(receipt["turn"].clone()).unwrap();
    let child = history(client, session).await;
    let run = child.runs.iter().find(|run| run.turn == turn).unwrap();
    assert_eq!(run.status, Status::Completed, "{run:?}");
    assert_eq!(
        &page
            .children
            .iter()
            .find(|child| child.run.turn == turn)
            .unwrap()
            .run,
        run
    );
    assert_eq!(
        admissions(client, page)
            .await
            .iter()
            .filter(|value| *value == receipt)
            .count(),
        1
    );
    received(
        parent,
        session,
        &format!("completed: {answer}"),
        child
            .runs
            .iter()
            .filter(|candidate| {
                candidate.status == Status::Completed && candidate.sequence <= run.sequence
            })
            .count(),
    );
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
pub(super) mod fixture;
mod guidance;
mod internal_guidance;
mod lifecycle;
mod parallel;
mod removal;
mod reuse;
mod roles;
mod selection;
mod streaming;

async fn capacity(client: &Client, name: &str, count: u64) {
    let Output::Plugin(package) = client
        .execute(client.prepare(Command::ReadPlugin { name: name.into() }))
        .await
        .unwrap()
    else {
        panic!("package expected")
    };
    let Output::PluginSettings(mut settings) = client
        .execute(client.prepare(Command::ReadPluginSettings {
            package: package.summary.reference(),
        }))
        .await
        .unwrap()
    else {
        panic!("settings expected")
    };
    settings
        .values
        .insert("child_concurrency".into(), json!(count.to_string()));
    client
        .execute(client.prepare(Command::SavePluginSettings {
            package: settings.package,
            values: settings.values,
            secrets: Default::default(),
        }))
        .await
        .unwrap();
}
