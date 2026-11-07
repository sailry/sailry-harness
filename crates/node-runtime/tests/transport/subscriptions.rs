//! Node, file and conversation subscriptions deliver the same snapshots and frames
//! through both Client transports, and a new subscription starts from a fresh snapshot.
use crate::file_fixture::Fixture;
use sailry_client::Client;
use sailry_link::Subscription;
use sailry_protocol::{
    conversation::{Change, Model, ModelApi, Provider, Queue},
    *,
};
use std::time::Duration;

async fn next(updates: &mut dyn Subscription) -> Update {
    tokio::time::timeout(Duration::from_secs(20), updates.next())
        .await
        .expect("subscription update deadline")
        .unwrap()
}

async fn snapshot(updates: &mut dyn Subscription) -> Snapshot {
    let Update::Snapshot(snapshot) = next(updates).await else {
        panic!("initial snapshot expected")
    };
    snapshot
}

async fn registered(updates: &mut dyn Subscription, project: ProjectId) -> EventEnvelope {
    loop {
        let update = next(updates).await;
        assert_ne!(
            update,
            Update::ResetRequired,
            "unexpected subscription reset"
        );
        if let Update::Event(envelope) = update
            && let Event::ProjectRegistered(published) = &envelope.event
            && published.id == project
        {
            return envelope;
        }
    }
}

#[tokio::test]
async fn delivers_node_snapshots_and_events() {
    let fixture = Fixture::empty().await;
    let [local, remote] = fixture.clients();
    let mut subscriptions = [
        local.subscribe().await.unwrap(),
        remote.subscribe().await.unwrap(),
    ];
    let initial = [
        snapshot(&mut *subscriptions[0]).await,
        snapshot(&mut *subscriptions[1]).await,
    ];
    assert_eq!(initial[0], initial[1]);
    // A durable change through either transport publishes one event to every subscriber.
    for (index, client) in [&local, &remote].into_iter().enumerate() {
        let root = fixture.root.join(format!("published-{index}"));
        std::fs::create_dir(&root).unwrap();
        let Output::Project(project) = client
            .execute(client.prepare(Command::RegisterProject {
                name: format!("Published {index}"),
                path: root.to_string_lossy().into_owned(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let published = [
            registered(&mut *subscriptions[0], project.id).await,
            registered(&mut *subscriptions[1], project.id).await,
        ];
        assert_eq!(published[0], published[1]);
    }
    drop(subscriptions);
    let Output::Snapshot(current) = local
        .execute(local.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    assert_eq!(current.projects.len(), initial[0].projects.len() + 2);
    for client in [&local, &remote] {
        let mut updates = client.subscribe().await.unwrap();
        assert_eq!(snapshot(&mut *updates).await.projects, current.projects);
    }
    fixture.close().await;
}

#[tokio::test]
async fn delivers_file_changes() {
    let fixture = Fixture::empty().await;
    let transports = [
        fixture.node.local(),
        fixture
            .controller
            .handle()
            .remote(fixture.node.link().address()),
    ];
    for (index, writer) in transports.iter().enumerate() {
        let mut watches = Vec::new();
        for transport in &transports {
            let mut watch = transport
                .subscribe(Topic::Files(fixture.worktree))
                .await
                .unwrap();
            next(&mut *watch).await;
            watches.push(watch);
        }
        let client = Client::new(writer.clone());
        client
            .execute(client.prepare(Command::WriteFile {
                worktree: fixture.worktree,
                path: format!("written-{index}.txt"),
                text: "changed".into(),
                expected_revision: None,
            }))
            .await
            .unwrap();
        for watch in &mut watches {
            assert_eq!(
                next(&mut **watch).await,
                Update::FilesChanged {
                    node: fixture.node.id(),
                    worktree: fixture.worktree,
                    references: 0,
                }
            );
        }
    }
    fixture.close().await;
}

async fn session(client: &Client) -> Session {
    let Output::Provider(provider) = client
        .execute(client.prepare(Command::PutProvider {
            expected_revision: 0,
            provider: Provider {
                request_max_retries: None,
                stream_max_retries: None,
                preset: None,
                binding: None,
                consent: None,
                id: ProviderId::new(),
                revision: 0,
                name: "Subscription fixture".into(),
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
        }))
        .await
        .unwrap()
    else {
        panic!("provider expected")
    };
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let Output::Session(session) = client
        .execute(client.prepare(Command::CreateSession {
            project: Some(snapshot.projects[0].id),
            worktree: None,
            config: Some(SessionConfig {
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
            }),
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    session
}

async fn queued(updates: &mut dyn Subscription, turn: TurnId) -> Queue {
    loop {
        match next(updates).await {
            Update::ConversationFrame(frame) => {
                if let Change::Queue(queue) = frame.change
                    && queue.items.iter().any(|item| item.turn == turn)
                {
                    return queue;
                }
            }
            Update::ResetRequired => panic!("unexpected conversation reset"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn delivers_conversation_frames() {
    let fixture = Fixture::empty().await;
    let [local, remote] = fixture.clients();
    let session = session(&local).await;
    let mut subscriptions = Vec::new();
    let mut pages = Vec::new();
    for client in [&local, &remote] {
        let mut updates = client.subscribe_conversation(session.id).await.unwrap();
        let Update::ConversationSnapshot(snapshot) = next(&mut *updates).await else {
            panic!("conversation snapshot expected")
        };
        pages.push(snapshot.page.clone());
        subscriptions.push(updates);
    }
    assert_eq!(pages[0], pages[1]);
    let Output::QueuedTurn(turn) = remote
        .execute(remote.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Queued through one transport".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("queued turn expected")
    };
    let mut queues = Vec::new();
    for updates in &mut subscriptions {
        queues.push(queued(&mut **updates, turn.id).await);
    }
    assert_eq!(queues[0], queues[1]);
    fixture.close().await;
}
