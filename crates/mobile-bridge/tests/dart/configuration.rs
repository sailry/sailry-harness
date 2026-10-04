use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{
    conversation::{Model, ModelApi, Provider},
    *,
};

#[allow(dead_code)]
#[path = "../../../node-runtime/tests/agent_support/mod.rs"]
pub(super) mod support;

pub(super) async fn provider(client: &Client, endpoint: &str, model: &str) -> Provider {
    let Output::Provider(provider) = client
        .execute(client.prepare(Command::SaveProvider {
            provider: Provider {
                options: None,
                id: ProviderId::new(),
                revision: 0,
                name: model.into(),
                api: ModelApi::ChatCompletions,
                authentication: sailry_protocol::Authentication::ApiKey,
                endpoint: endpoint.into(),
                enabled: true,
                credential: None,
                default_model: model.into(),
                models: vec![Model {
                    id: model.into(),
                    context: 4096,
                    output: 128,
                    vision: false,
                    tools: false,
                    reasoning: false,
                    web_search: false,
                    generates: vec![],
                    efforts: Vec::new(),
                    custom_efforts: false,
                    default_effort: sailry_protocol::Effort::Default,
                }],
            },
            expected_revision: 0,
            secret: Some(Secret::new("isolated-ffi-configuration-credential".into())),
        }))
        .await
        .unwrap()
    else {
        panic!("provider expected")
    };
    provider
}

pub(super) fn config(provider: &Provider) -> SessionConfig {
    SessionConfig {
        assistant: None,
        resource: None,
        provider: provider.id,
        model: provider.default_model.clone(),
        effort: Effort::Low,
        mode: sailry_protocol::WorkMode::Code,
        permission: sailry_protocol::Permission::Ask,
        credential: provider.credential.clone(),
    }
}

async fn role(client: &Client, provider: &Provider, key: &str) {
    client
        .execute(client.prepare(Command::PutRole {
            expected_revision: 0,
            role: role::Profile {
                appearance: None,
                id: RoleId::new(),
                revision: 0,
                key: key.into(),
                name: "Review 中文 🙂".into(),
                description: "Review the delegated task".into(),
                model: Some(role::Model {
                    provider: provider.id,
                    model: provider.default_model.clone(),
                    effort: None,
                }),
                max_turns: Some(12),
                skills: vec!["fixture/review".into()],
                instructions: "Preserve the original instructions\n完整内容 🙂".into(),
            },
        }))
        .await
        .unwrap();
}

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn resumes_without_source() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let selected = runtime.block_on(support::Server::start(false));
    let defaults = runtime.block_on(support::Server::start(false));
    let (target, session) = runtime.block_on(async {
        let source = Node::start(directory.path().join("source")).await.unwrap();
        let target = Node::start(directory.path().join("target")).await.unwrap();
        source
            .link()
            .pair(target.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let origin = Client::new(source.local());
        let destination = Client::new(target.local());
        let selected = provider(&origin, &selected.endpoint, "ffi-source").await;
        let defaults = provider(&destination, &defaults.endpoint, "ffi-default").await;
        role(&origin, &selected, "source-review").await;
        role(&destination, &defaults, "target-review").await;
        destination
            .execute(destination.prepare(Command::SetDefaults {
                expected_revision: 0,
                config: config(&defaults),
            }))
            .await
            .unwrap();
        let Output::Project(project) = destination
            .execute(destination.prepare(Command::RegisterProject {
                name: "FFI configuration".into(),
                path: directory.path().to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Session(session) = origin
            .execute(origin.prepare(Command::CreateSessionAt {
                target: target.id(),
                project: Some(project.id),
                worktree: None,
                config: Box::new(config(&selected)),
                provider_revision: selected.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        source.shutdown().await.unwrap();
        (target, session)
    });
    let invitation = target.link().invite().unwrap();
    let child = super::command("configuration.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSION", session.id.to_string())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let expected = runtime.block_on(async {
        let client = Client::new(target.local());
        let result = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap();
        target.shutdown().await.unwrap();
        result
    });
    assert!(status.success());
    runtime.block_on(async {
        let target = Node::start(directory.path().join("target")).await.unwrap();
        let client = Client::new(target.local());
        let Output::Snapshot(before) = expected else {
            panic!("snapshot expected")
        };
        let Output::Snapshot(after) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(after.sessions, before.sessions);
        assert_eq!(after.turns, before.turns);
        assert_eq!(after.roles, before.roles);
        assert_eq!(after.turns.len(), 4);
        assert_eq!(after.turns[2].roles.profiles[0].key, "source-review");
        assert_eq!(after.turns[3].roles.profiles[0].key, "target-review");
        target.shutdown().await.unwrap();
    });
    assert_eq!(selected.requests.lock().unwrap().len(), 1);
    assert_eq!(defaults.requests.lock().unwrap().len(), 1);
    assert_eq!(
        selected.authorization.lock().unwrap()[0].as_deref(),
        Some("Bearer isolated-ffi-configuration-credential")
    );
}
