use super::*;
use sailry_protocol::conversation::cloud::Options;
#[path = "cloud/requests.rs"]
mod requests;

const APIS: &[ModelApi] = &[ModelApi::AzureOpenAi, ModelApi::AzureAi];

pub(super) fn options(api: ModelApi) -> Option<Options> {
    match api {
        ModelApi::AzureOpenAi => Some(Options::AzureOpenAi {
            api_version: "2024-10-21".into(),
        }),
        _ => None,
    }
}

#[tokio::test]
async fn restores_frozen_requests() {
    for &api in APIS {
        for remote in [false, true] {
            let server = Server::start(api, Reply::Text).await;
            let fixture = Fixture::new(remote, api, &server.endpoint).await;
            let turn = fixture.submit("Cloud fixture 🙂").await;
            let history_before = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                history_before.runs.last().unwrap().status,
                Status::Completed,
                "{api:?}: {:?}",
                history_before.runs
            );
            assert!(text(&history_before).contains(server::ANSWER));
            let Output::QueuedTurn(queued) = execute(
                &fixture.client,
                Command::QueueTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: "Continue".into(),
                },
            )
            .await
            else {
                panic!("queued turn expected")
            };
            let mut changed = fixture.provider.clone();
            changed.endpoint = "http://127.0.0.1:9/changed".into();
            changed.models[0].output = 16;
            if let Some(options) = &mut changed.options {
                match options {
                    Options::AzureOpenAi { api_version } => *api_version = "2020-01-01".into(),
                }
            }
            execute(
                &fixture.client,
                Command::PutProvider {
                    expected_revision: changed.revision,
                    provider: changed,
                },
            )
            .await;
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
            let node = Node::start(fixture.directory.path().join("node"))
                .await
                .unwrap();
            let controller = Link::controller(
                fixture.directory.path().join("resumed"),
                NetworkScope::default(),
            )
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
            assert_eq!(
                history(&client, fixture.session.id).await.entries,
                history_before.entries
            );
            execute(&client, Command::StartQueuedTurn { turn: queued.id }).await;
            let page = finished(&client, queued.session, queued.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{api:?}: {:?}",
                page.runs
            );
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 2, "{api:?}");
            for request in requests {
                assert!(!request.body.to_string().contains(server::KEY));
                match api {
                    ModelApi::AzureOpenAi => {
                        assert_eq!(
                            request.path,
                            "/v1/openai/deployments/fixture-a/chat/completions?api-version=2024-10-21"
                        );
                        assert_eq!(request.headers["api-key"], server::KEY);
                        assert!(!request.headers.contains_key("authorization"));
                        assert_eq!(request.body["reasoning_effort"], "high");
                        assert_eq!(request.body["max_completion_tokens"], 512);
                    }
                    ModelApi::AzureAi => {
                        assert_eq!(
                            request.path,
                            "/v1/chat/completions?api-version=2024-05-01-preview"
                        );
                        assert_eq!(request.headers["api-key"], server::KEY);
                        assert_eq!(request.body["model"], "fixture-a");
                        assert_eq!(request.body["max_tokens"], 512);
                    }
                    _ => unreachable!(),
                }
            }
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn records_usage_and_cancels_requests() {
    for &api in APIS {
        for remote in [false, true] {
            let server = Server::start(api, Reply::Usage).await;
            let fixture = Fixture::new(remote, api, &server.endpoint).await;
            let turn = fixture.submit("Usage fixture").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            let usages = page
                .entries
                .iter()
                .filter_map(|entry| entry.usage.clone())
                .collect::<Vec<_>>();
            assert_eq!(
                usages,
                vec![Usage {
                    input: 16,
                    output: 4,
                    cached_input: 6,
                    reasoning: 1
                }],
                "{api:?}"
            );
            let provider = fixture.provider.clone();
            let closed = Arc::new(tokio::sync::Notify::new());
            let stalled = Server::start(api, Reply::Wait(closed.clone())).await;
            let mut provider = provider;
            provider.endpoint = stalled.endpoint.clone();
            execute(
                &fixture.client,
                Command::PutProvider {
                    expected_revision: provider.revision,
                    provider,
                },
            )
            .await;
            let turn = fixture.submit("Cancellation fixture").await;
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while stalled.requests.lock().unwrap().is_empty() {
                assert!(std::time::Instant::now() < deadline);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            execute(&fixture.client, Command::StopTurn { turn }).await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
            tokio::time::timeout(Duration::from_secs(5), closed.notified())
                .await
                .unwrap();
            assert_eq!(stalled.requests.lock().unwrap().len(), 1);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}
