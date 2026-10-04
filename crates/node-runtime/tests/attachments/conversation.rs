use super::*;
use sailry_protocol::conversation::{Input, Model, ModelApi, Page, Part, Provider, Status};

#[path = "conversation/inputs.rs"]
mod inputs;
#[path = "conversation/media.rs"]
mod media;
#[path = "conversation/model.rs"]
mod model;
#[path = "conversation/native.rs"]
mod native;
#[allow(dead_code)]
#[path = "../agent/providers/native/server.rs"]
mod provider;
#[path = "conversation/queue.rs"]
mod queue;
use crate::agent_support as server;
#[path = "conversation/tool_images.rs"]
mod tool_images;

async fn session(
    client: &Client,
    worktree: WorktreeId,
    endpoint: &str,
    api: ModelApi,
) -> (Session, Provider) {
    let provider = Provider {
        options: match api {
            ModelApi::AzureOpenAi => {
                Some(sailry_protocol::conversation::cloud::Options::AzureOpenAi {
                    api_version: "2024-10-21".into(),
                })
            }
            ModelApi::Bedrock => Some(sailry_protocol::conversation::cloud::Options::Bedrock {
                region: "us-east-1".into(),
            }),
            ModelApi::Vertex => Some(sailry_protocol::conversation::cloud::Options::Vertex {
                project: "fixture-project".into(),
                location: "global".into(),
            }),
            _ => None,
        },
        id: ProviderId::new(),
        revision: 0,
        name: "Attachment fixture".into(),
        api,
        authentication: sailry_protocol::Authentication::ApiKey,
        endpoint: endpoint.into(),
        enabled: true,
        default_model: "fixture".into(),
        credential: None,
        models: vec![Model {
            id: "fixture".into(),
            context: 128000,
            output: 64,
            vision: true,
            tools: api == ModelApi::Bedrock,
            reasoning: false,
            web_search: false,
            generates: vec![],
            efforts: vec![],
            custom_efforts: false,
            default_effort: sailry_protocol::Effort::Default,
        }],
    };
    let command = if matches!(
        api,
        ModelApi::AzureOpenAi | ModelApi::AzureAi | ModelApi::Bedrock | ModelApi::Vertex
    ) {
        Command::SaveProvider {
            provider,
            expected_revision: 0,
            secret: Some(sailry_protocol::Secret::new(
                "attachment-fixture-key".into(),
            )),
        }
    } else {
        Command::PutProvider {
            provider,
            expected_revision: 0,
        }
    };
    let Output::Provider(provider) = client.execute(client.prepare(command)).await.unwrap() else {
        panic!("provider expected")
    };
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let project = snapshot
        .worktrees
        .iter()
        .find(|entry| entry.id == worktree)
        .unwrap()
        .project;
    let Output::Session(session) = client
        .execute(client.prepare(Command::CreateSession {
            project,
            worktree: Some(worktree),
            config: Some(SessionConfig {
                assistant: None,
                resource: None,
                provider: provider.id,
                model: "fixture".into(),
                effort: Effort::Medium,
                mode: sailry_protocol::WorkMode::Code,
                permission: Permission::Ask,
                credential: provider.credential.clone(),
            }),
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    (session, provider)
}

async fn upload(
    client: &Client,
    worktree: WorktreeId,
    name: &str,
    media: &str,
    data: &[u8],
) -> Attachment {
    let mut metadata = spec(worktree, name, data);
    metadata.media_type = media.into();
    let pending = prepare(client, metadata).await;
    stage(client, &pending, data).await;
    publish(client, &pending).await
}

fn input(text: &str, attachments: &[&Attachment]) -> Input {
    Input {
        references: Vec::new(),
        text: text.into(),
        attachments: attachments.iter().map(|attachment| attachment.id).collect(),
    }
}

async fn queue(client: &Client, session: &Session, message: Input) -> QueuedTurn {
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message,
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn
}

async fn page(client: &Client, session: SessionId) -> Page {
    let Output::Conversation(history) = client
        .execute(client.prepare(Command::ReadConversation {
            session,
            before: None,
            limit: 100,
        }))
        .await
        .unwrap()
    else {
        panic!("history expected")
    };
    history.page
}

async fn run(client: &Client, turn: &QueuedTurn) -> Page {
    client
        .execute(client.prepare(Command::StartQueuedTurn { turn: turn.id }))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let page = page(client, turn.session).await;
            if page.runs.iter().any(|run| {
                run.turn == turn.id
                    && !matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
            }) {
                return page;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("attachment turn deadline")
}

fn discard(client: &Client, attachment: &Attachment) -> Request {
    client.prepare(Command::DiscardAttachment {
        worktree: attachment.spec.worktree,
        attachment: attachment.id,
    })
}

fn png() -> Vec<u8> {
    use image::ImageEncoder;
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&[128, 64, 32, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    bytes
}

fn pdf() -> Vec<u8> {
    include_bytes!("../../../../tests/fixtures/document.pdf").to_vec()
}
