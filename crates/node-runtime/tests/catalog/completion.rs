use super::*;
use sailry_protocol::conversation::{Model, Provider};

fn provider() -> Provider {
    Provider {
        oauth: None,
        id: ProviderId::new(),
        revision: 0,
        name: "Reference fixture".into(),
        api: ModelApi::Responses,
        authentication: Authentication::ApiKey,
        endpoint: "https://api.openai.com/v1".into(),
        options: None,
        enabled: true,
        credential: None,
        default_model: "gpt-5.6-luna".into(),
        models: vec![Model {
            id: "gpt-5.6-luna".into(),
            context: 200_000,
            output: 8192,
            vision: false,
            tools: true,
            reasoning: true,
            web_search: false,
            generates: vec![],
            efforts: vec![Effort::Default],
            custom_efforts: false,
            default_effort: Effort::Default,
        }],
    }
}

async fn save(client: &Client, provider: Provider) -> Provider {
    let Output::Provider(provider) = execute(
        client,
        Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    provider
}

#[tokio::test]
async fn fills_missing_choices() {
    let catalog = json!({
        "openai": {"models": {"gpt-5.6-luna": {
            "id": "gpt-5.6-luna", "reasoning": true,
            "limit": {"context": 1_000_000, "output": 128_000},
            "reasoning_options": [{"type":"effort","values":["none","low","medium","high","xhigh","max"]}]
        }}},
        "gateway": {"models": {"gpt-5.6-luna": {
            "id": "gpt-5.6-luna", "reasoning": true,
            "reasoning_options": [{"type":"effort","values":["high","medium","low"]}]
        }}}
    });
    for remote in [false, true] {
        let data = catalog.clone();
        let server = Server::start(ModelApi::Responses, move |_| Reply::Json(data.clone())).await;
        let fixture = Fixture::new(remote, &server).await;
        let original = save(&fixture.client, provider()).await;
        assert!(original.models[0].web_search);
        assert_eq!(original.models[0].efforts, [Effort::Default]);
        let mut custom = provider();
        custom.endpoint = "https://gateway.example/v1".into();
        custom.models[0].efforts = vec![Effort::High];
        custom.models[0].default_effort = Effort::High;
        let custom = save(&fixture.client, custom).await;
        let mut manual = provider();
        manual.models[0].custom_efforts = true;
        let manual = save(&fixture.client, manual).await;
        let mut subscription = fixture.client.subscribe().await.unwrap();
        let mut projection = Projection::new(fixture.client.target(), 1);
        fixture.client.recover(&mut projection, 1).await.unwrap();
        let status = refresh(&fixture.client).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            while projection.snapshot().unwrap().model_catalog != status {
                projection
                    .apply(1, subscription.next().await.unwrap())
                    .unwrap();
            }
        })
        .await
        .unwrap();
        let snapshot = projection.snapshot().unwrap();
        let completed = snapshot
            .providers
            .iter()
            .find(|value| value.id == original.id)
            .unwrap();
        assert_eq!(completed.revision, original.revision + 1);
        assert_eq!(
            completed.models[0].efforts,
            [
                Effort::Disabled,
                Effort::Low,
                Effort::Medium,
                Effort::High,
                Effort::XHigh,
                Effort::Max
            ]
        );
        assert_eq!(
            (completed.models[0].context, completed.models[0].output),
            (200_000, 8192)
        );
        assert_eq!(completed.models[0].default_effort, Effort::Disabled);
        assert_eq!(
            snapshot
                .providers
                .iter()
                .find(|value| value.id == manual.id),
            Some(&manual)
        );
        let retained = save(&fixture.client, manual.clone()).await;
        assert_eq!(retained.models, manual.models);
        assert_eq!(
            snapshot
                .providers
                .iter()
                .find(|value| value.id == custom.id),
            Some(&custom)
        );
        let saved = save(&fixture.client, provider()).await;
        assert_eq!(saved.models[0].efforts, completed.models[0].efforts);
        let mut gateway = provider();
        gateway.endpoint = "https://gateway.example/v1".into();
        let gateway = save(&fixture.client, gateway).await;
        assert_eq!(
            gateway.models[0].efforts,
            [Effort::High, Effort::Medium, Effort::Low]
        );
        assert!(gateway.models[0].web_search);
        let mut unknown = provider();
        unknown.endpoint = gateway.endpoint.clone();
        unknown.models[0].id = "unknown-model".into();
        unknown.default_model = "unknown-model".into();
        let unknown = save(&fixture.client, unknown).await;
        assert!(!unknown.models[0].web_search);
        let Fixture {
            _directory,
            node,
            controller,
            client,
        } = fixture;
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let node = Node::start_with_catalog(
            _directory.path().join("node"),
            &format!("{}/api.json", server.endpoint),
        )
        .await
        .unwrap();
        let client = Client::new(node.local());
        refresh(&client).await;
        let Output::Providers(providers) = execute(&client, Command::ListProviders).await else {
            panic!("providers expected")
        };
        let reopened = providers
            .iter()
            .find(|value| value.id == manual.id)
            .unwrap();
        assert_eq!(reopened.models, manual.models);
        node.shutdown().await.unwrap();
    }
}
