use super::*;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{
    conversation::{Provider, Status},
    *,
};

#[tokio::test]
async fn publishes_admitted_retries() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = std::sync::Arc::new(Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        }));
        let Output::Project(project) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Request fixture".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        for mode in ["standard", "compatible", "max"] {
            for status in [429, 503] {
                let server =
                    Server::start_with_request(ModelApi::Anthropic, move |_| failed(status)).await;
                let provider = Provider {
                    options: None,
                    id: ProviderId::new(),
                    revision: 0,
                    name: "Request fixture".into(),
                    api: ModelApi::Responses,
                    authentication: Authentication::ApiKey,
                    endpoint: server.endpoint.clone(),
                    enabled: true,
                    models: vec![],
                    default_model: "fixture".into(),
                    credential: None,
                };
                let Output::Provider(provider) = client
                    .execute(client.prepare(Command::SaveProvider {
                        provider,
                        expected_revision: 0,
                        secret: (mode != "compatible").then(|| Secret::new("isolated-key".into())),
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("provider expected")
                };
                let Output::Session(session) = client
                    .execute(client.prepare(Command::CreateSession {
                        project: Some(project.id),
                        worktree: None,
                        config: Some(SessionConfig {
                            assistant: None,
                            resource: None,
                            provider: provider.id,
                            model: "fixture".into(),
                            effort: if mode == "max" {
                                Effort::Max
                            } else {
                                Effort::Default
                            },
                            mode: sailry_protocol::WorkMode::Code,
                            permission: Permission::Ask,
                            credential: provider.credential,
                        }),
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("session expected")
                };
                let request = client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: "Isolated request".into(),
                });
                let (updates, mut observed) =
                    tokio::sync::watch::channel(sailry_client::conversation::View::default());
                let stop = sailry_link::CancellationToken::new();
                let (_paging, requests) = tokio::sync::mpsc::channel(1);
                let watching = {
                    let client = client.clone();
                    let stop = stop.clone();
                    tokio::spawn(async move {
                        client
                            .watch_conversation(session.id, updates, stop, requests)
                            .await
                    })
                };
                let output = client.execute(request.clone()).await.unwrap();
                let Output::QueuedTurn(turn) = &output else {
                    panic!("turn expected")
                };
                tokio::time::timeout(Duration::from_secs(15), async {
                    loop {
                        let Output::Conversation(history) = client
                            .execute(client.prepare(Command::ReadConversation {
                                session: session.id,
                                before: None,
                                limit: 10,
                            }))
                            .await
                            .unwrap()
                        else {
                            panic!("history expected")
                        };
                        if let Some(run) = history.page.runs.iter().find(|run| run.turn == turn.id)
                            && run.status == Status::Failed
                        {
                            let retries = history.page.entries.iter().flat_map(|entry| &entry.parts)
                                .filter(|part| matches!(part, sailry_protocol::conversation::Part::Resource(value) if value["type"] == "model_retry"))
                                .count();
                            assert_eq!(retries, 5);
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let view = observed.borrow_and_update().clone();
                        assert!(view.error.is_none(), "observer failed: {:?}", view.error);
                        if let Some(snapshot) = view.snapshot
                            && snapshot
                                .page
                                .runs
                                .iter()
                                .any(|run| run.turn == turn.id && run.status == Status::Failed)
                        {
                            let attempts: Vec<_> = snapshot
                                .page
                                .entries
                                .iter()
                                .flat_map(|entry| &entry.parts)
                                .filter_map(|part| match part {
                                    conversation::Part::Resource(value)
                                        if value["type"] == "model_retry" =>
                                    {
                                        value["attempt"].as_u64()
                                    }
                                    _ => None,
                                })
                                .collect();
                            assert_eq!(attempts, [1, 2, 3, 4, 5]);
                            break;
                        }
                        observed.changed().await.unwrap();
                    }
                })
                .await
                .unwrap();
                stop.cancel();
                watching.await.unwrap().unwrap();
                assert_eq!(client.execute(request).await.unwrap(), output);
                assert_eq!(
                    server.requests.lock().unwrap().len(),
                    6,
                    "{mode}, remote={remote}, status={status}"
                );
            }
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
