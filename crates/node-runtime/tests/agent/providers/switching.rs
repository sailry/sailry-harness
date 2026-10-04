use super::*;

#[tokio::test]
async fn retains_reasoning_across_apis() {
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
        let transport: Arc<dyn Transport> = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport);
        let first = Server::reasoning().await;
        let second = Server::start(false).await;
        let (session, mut provider) = configured(&client, &first.endpoint, &root).await;
        provider.api = ModelApi::Responses;
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
        let reasoning = requests[0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| {
                message["reasoning_content"] == "Retained reasoning without a final answer"
            })
            .expect("reasoning retained across model switch");
        assert_eq!(reasoning["content"], "");
        assert_eq!(requests[0]["reasoning_effort"], "high");
        assert!(
            serde_json::to_string(&page)
                .unwrap()
                .contains("Retained reasoning without a final answer")
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
