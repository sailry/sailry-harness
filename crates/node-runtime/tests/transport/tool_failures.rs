use crate::{agent_support::Server, file_fixture::Fixture};
use sailry_client::{Client, conversation::Projection};
use sailry_protocol::{
    conversation::{ModelApi, Part, Provider, Status},
    tool::Failure,
    *,
};
use serde_json::json;
use std::time::Duration;

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

#[tokio::test]
async fn preserves_origins_through_conversation_pages() {
    let model = Server::batch(vec![
        ("set_session_title".into(), json!({"title":"\n"})),
        ("unregistered_fixture_tool".into(), json!({})),
    ])
    .await;
    let fixture = Fixture::empty().await;
    let clients = fixture.clients();
    let Output::Provider(provider) = execute(
        &clients[0],
        Command::PutProvider {
            expected_revision: 0,
            provider: Provider {
                request_max_retries: None,
                stream_max_retries: None,
                preset: None,
                binding: None,
                consent: None,
                oauth: None,
                options: None,
                id: ProviderId::new(),
                revision: 0,
                name: "Failure fixture".into(),
                api: ModelApi::ChatCompletions,
                authentication: Authentication::ApiKey,
                endpoint: model.endpoint.clone(),
                enabled: true,
                models: Vec::new(),
                default_model: String::new(),
                credential: None,
            },
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    for client in &clients {
        let Output::Session(session) = execute(
            client,
            Command::CreateSession {
                project: None,
                worktree: None,
                config: Some(SessionConfig {
                    features: Default::default(),
                    assistant: None,
                    resource: None,
                    provider: provider.id,
                    model: "fixture-a".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    options: Default::default(),
                    permission: Permission::Ask,
                    credential: None,
                }),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let mut updates = client.subscribe_conversation(session.id).await.unwrap();
        let mut projection = Projection::new(client.target(), session.id, 0);
        let Output::QueuedTurn(turn) = execute(
            client,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Exercise tool failure presentation".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                projection.apply(0, updates.next().await.unwrap()).unwrap();
                if projection.snapshot().is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.turn == turn.id && run.status == Status::Completed)
                }) {
                    break;
                }
            }
        })
        .await
        .unwrap();
        let expected = [
            Failure::tool(Some(ErrorCode::InvalidRequest)),
            Failure::runtime(None),
        ];
        let projected = &projection.snapshot().unwrap().page;
        for reader in &clients {
            let Output::Conversation(history) = execute(
                reader,
                Command::ReadConversation {
                    session: session.id,
                    before: None,
                    limit: 100,
                },
            )
            .await
            else {
                panic!("conversation expected")
            };
            let results: Vec<_> = history
                .page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| match part {
                    Part::ToolResult { summary, .. } => Some(summary),
                    _ => None,
                })
                .collect();
            assert_eq!(results.len(), expected.len());
            for (summary, failure) in results.iter().zip(&expected) {
                assert_eq!(summary.failure.as_ref(), Some(failure));
                assert!(
                    summary
                        .error
                        .as_ref()
                        .is_some_and(|error| !error.is_empty())
                );
            }
            assert_eq!(&history.page, projected.as_ref());
        }
    }
    fixture.close().await;
}
