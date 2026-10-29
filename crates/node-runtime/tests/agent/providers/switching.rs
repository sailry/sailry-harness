use super::*;

#[tokio::test]
async fn isolates_reasoning_across_apis() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transport: Arc<dyn Transport> = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport);
        let first = Server::reasoning().await;
        let second = Server::start(false).await;
        let (session, mut provider) = configured_session(&client, &first.endpoint, &root).await;
        provider.api = ModelApi::Responses;
        provider.default_model = "fixture-a".into();
        provider.models = ["fixture-a", "fixture-b"]
            .into_iter()
            .map(|id| Model {
                id: id.into(),
                display_name: None,
                context: 128_000,
                output: 2048,
                vision: false,
                tools: true,
                reasoning: true,
                generates: Vec::new(),
                efforts: vec![Effort::Low, Effort::High],
                capabilities: Some(sailry_protocol::conversation::discovery::Capabilities {
                    reasoning: Some(true),
                    tools: Some(true),
                    ..Default::default()
                }),
                custom_efforts: false,
                compaction_tokens: None,
                default_effort: Effort::High,
            })
            .collect();
        client
            .execute(client.prepare(Command::PutProvider {
                provider: provider.clone(),
                expected_revision: 1,
            }))
            .await
            .unwrap();
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: 1,
                message: "Think about this task".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        finished(&client, session.id, turn.id).await;
        provider.id = ProviderId::new();
        provider.revision = 0;
        provider.api = ModelApi::DeepSeek;
        for model in &mut provider.models {
            model.efforts = vec![Effort::High];
        }
        provider.endpoint = second.endpoint.clone();
        client
            .execute(client.prepare(Command::PutProvider {
                provider: provider.clone(),
                expected_revision: 0,
            }))
            .await
            .unwrap();
        let mut config = session.config.clone();
        config.provider = provider.id;
        config.model = "fixture-b".into();
        client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: 1,
                config,
            }))
            .await
            .unwrap();
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: 2,
                message: "Continue".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed
        );
        let requests = second.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert!(
            !requests[0]
                .to_string()
                .contains("Retained reasoning without a final answer")
        );
        assert!(
            requests[0]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .all(|message| message.get("reasoning_content").is_none())
        );
        assert_eq!(requests[0]["reasoning_effort"], "high");
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part, Part::Thinking(value) if value == "Retained reasoning without a final answer")));
        let first_requests = {
            let requests = first.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0]["reasoning"]["effort"], "high");
            requests.len()
        };
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&client, session.id).await, page);
        assert_eq!(first.requests.lock().unwrap().len(), first_requests);
        assert_eq!(second.requests.lock().unwrap().len(), 1);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
