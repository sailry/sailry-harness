use crate::file_fixture::Fixture;
use sailry_client::{Apply, Client, Projection};
use sailry_protocol::{
    conversation::{Model, ModelApi, Provider},
    *,
};
use std::time::Duration;

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn snapshot(client: &Client) -> Snapshot {
    let Output::Snapshot(snapshot) = execute(client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    snapshot
}

async fn configure(client: &Client) -> SessionConfig {
    let Output::Provider(provider) = execute(
        client,
        Command::PutProvider {
            expected_revision: 0,
            provider: Provider {
                request_max_retries: None,
                stream_max_retries: None,
                preset: None,
                binding: None,
                consent: None,
                id: ProviderId::new(),
                revision: 0,
                name: "Snapshot fixture".into(),
                api: ModelApi::ChatCompletions,
                authentication: Authentication::ApiKey,
                endpoint: "http://127.0.0.1:9/v1".into(),
                options: None,
                oauth: None,
                enabled: true,
                default_model: "fixture".into(),
                credential: None,
                models: vec![Model {
                    id: "fixture".into(),
                    display_name: None,
                    context: 128,
                    output: 32,
                    vision: false,
                    tools: true,
                    reasoning: false,
                    generates: vec![],
                    efforts: vec![],
                    capabilities: None,
                    custom_efforts: false,
                    compaction_tokens: None,
                    default_effort: Effort::Default,
                }],
            },
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    let Output::Roles(roles) = execute(client, Command::ListRoles).await else {
        panic!("roles expected")
    };
    for role in roles {
        execute(
            client,
            Command::RemoveRole {
                role: role.id,
                expected_revision: role.revision,
            },
        )
        .await;
    }
    let suffix = "中文 🙂";
    for index in 0..33 {
        execute(
            client,
            Command::PutRole {
                expected_revision: 0,
                role: role::Profile {
                    id: RoleId::new(),
                    revision: 0,
                    key: format!("role-{index:02}"),
                    name: format!("Role {index}"),
                    appearance: None,
                    description: String::new(),
                    read_only: false,
                    model: None,
                    max_turns: None,
                    skills: vec![],
                    tools: None,
                    instructions: "x".repeat(16 * 1024 - suffix.len()) + suffix,
                },
            },
        )
        .await;
    }
    SessionConfig {
        features: Default::default(),
        assistant: None,
        resource: None,
        provider: provider.id,
        model: "fixture".into(),
        effort: Effort::Default,
        mode: WorkMode::Code,
        options: Default::default(),
        permission: Permission::Ask,
        credential: None,
    }
}

async fn create(client: &Client, project: ProjectId, config: &SessionConfig) -> Session {
    let Output::Session(session) = execute(
        client,
        Command::CreateSession {
            project: Some(project),
            worktree: None,
            config: Some(config.clone()),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    session
}

async fn observe(client: &Client, expected: &Snapshot) {
    let mut updates = client.subscribe().await.unwrap();
    let initial = tokio::time::timeout(Duration::from_secs(20), updates.next())
        .await
        .unwrap()
        .unwrap();
    let mut projection = Projection::new(expected.node, 1);
    assert_eq!(projection.apply(1, initial).unwrap(), Apply::Applied);
    assert_eq!(projection.snapshot(), Some(expected));
    projection.reconnect(2).unwrap();
    // Recovery executes Command::Snapshot through the same authenticated client.
    client.recover(&mut projection, 2).await.unwrap();
    assert_eq!(projection.snapshot(), Some(expected));
}

#[tokio::test]
async fn preserves_large_history_and_sessions() {
    let fixture = Fixture::empty().await;
    let [local, remote] = fixture.clients();
    let config = configure(&local).await;
    let project = snapshot(&local).await.projects[0].id;
    let session = create(&local, project, &config).await;
    let pending = remote.prepare(Command::QueueTurn {
        session: session.id,
        expected_revision: session.revision,
        message: "First captured input".into(),
    });
    // Lose completion observation after durable receipt, retaining the original request.
    let admission = remote.dispatch(pending.clone()).await.unwrap();
    assert!(admission.receipt.durable);
    drop(admission);
    for index in 1..34 {
        execute(
            &local,
            Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: format!("Queued input {index}").into(),
            },
        )
        .await;
    }
    let history = snapshot(&local).await;
    assert_eq!(history.turns.len(), 34);
    // Role captures remain stored once per admission, not repeated in the index.
    assert!(serde_json::to_vec(&history.turns).unwrap().len() < 16 * 1024);
    for client in [&local, &remote] {
        observe(client, &history).await;
    }
    let RequestOutcome::Completed(result) = remote.outcome(&pending).await.unwrap() else {
        panic!("completed original request expected")
    };
    let Output::QueuedTurn(first) = result.unwrap() else {
        panic!("queued turn expected")
    };
    assert_eq!(history.turns[0], TurnSummary::from(&first));
    for client in [&local, &remote] {
        assert_eq!(
            client
                .read_turn_capture(session.id, first.id)
                .await
                .unwrap(),
            first
        );
    }
    let mut changed = session.roles.profiles[0].clone();
    changed.instructions = "Updated role".into();
    execute(
        &local,
        Command::PutRole {
            expected_revision: changed.revision,
            role: changed,
        },
    )
    .await;
    // Session growth independently exceeds one frame even with all turn records omitted.
    for _ in 0..34 {
        create(&local, project, &config).await;
    }
    let catalog = snapshot(&local).await;
    assert_eq!(catalog.sessions.len(), 35);
    assert!(serde_json::to_vec(&catalog.sessions).unwrap().len() > MAX_FRAME_BYTES);
    for client in [&local, &remote] {
        observe(client, &catalog).await;
        let Output::QueuedMessage(queued) =
            execute(client, Command::ReadQueuedTurn { turn: first.id }).await
        else {
            panic!("queued message expected")
        };
        assert_eq!(queued.turn, first);
    }
    assert_eq!(
        remote.outcome(&pending).await.unwrap(),
        RequestOutcome::Completed(Box::new(Ok(Output::QueuedTurn(first))))
    );
    fixture.close().await;
}
