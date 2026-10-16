use super::*;
use sailry_protocol::conversation::connection::{Options, Product};

fn provider(api: ModelApi, options: Option<Options>, endpoint: &str) -> Provider {
    Provider {
        id: ProviderId::new(),
        revision: 0,
        name: "Connection fixture".into(),
        api,
        authentication: Authentication::ApiKey,
        endpoint: endpoint.into(),
        options,
        oauth: None,
        enabled: true,
        models: vec![],
        default_model: String::new(),
        credential: None,
    }
}

async fn providers(client: &Client) -> Vec<Provider> {
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    snapshot.providers
}

#[tokio::test]
async fn persists_explicit_products_without_rewriting_existing() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let profile = fixture.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        let controller =
            Link::controller(fixture.path().join("controller"), NetworkScope::default())
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
        let original = provider(
            ModelApi::ChatCompletions,
            None,
            "https://api.minimax.chat/v1",
        );
        let Output::Provider(original) = client
            .execute(client.prepare(Command::SaveProvider {
                expected_revision: 0,
                provider: original,
                secret: Some(Secret::new("isolated-connection-key".into())),
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        assert_eq!(original.endpoint, "https://api.minimax.chat/v1");
        assert!(original.options.is_none());
        assert!(original.credential.is_some());
        for api in [
            ModelApi::ChatCompletions,
            ModelApi::Anthropic,
            ModelApi::DeepSeek,
        ] {
            let mut selected = provider(api, None, "https://example.test/custom");
            for product in [Product::Metered, Product::Coding] {
                selected.options = Some(Options::Hosted { product });
                let Output::Provider(saved) = client
                    .execute(client.prepare(Command::PutProvider {
                        expected_revision: selected.revision,
                        provider: selected.clone(),
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("provider expected")
                };
                selected.revision += 1;
                assert_eq!(saved, selected);
                let current = providers(&client).await;
                assert!(current.contains(&selected));
                assert!(current.contains(&original));
            }
            let before = providers(&client).await;
            let mut invalid = selected;
            invalid.api = ModelApi::Responses;
            assert_eq!(
                client
                    .execute(client.prepare(Command::PutProvider {
                        expected_revision: invalid.revision,
                        provider: invalid,
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
            assert_eq!(providers(&client).await, before);
        }
        let before = providers(&client).await;
        node.shutdown().await.unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(providers(&client).await, before);
        assert!(providers(&client).await.contains(&original));
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
