use crate::{
    discovery_support::{Reply, Server},
    file_fixture::Fixture,
};
use sailry_client::Projection;
use sailry_protocol::{
    conversation::{Model, ModelApi, Provider, catalog},
    *,
};
use std::time::Duration;

#[tokio::test]
async fn preserves_reads_and_status_projection() {
    let source = Server::start(ModelApi::Anthropic, |_| {
        Reply::Json(serde_json::from_str(include_str!("../fixtures/model-scores.json")).unwrap())
    })
    .await;
    let fixture = Fixture::empty().await;
    fixture
        .node
        .set_model_scores_fixture(&source.endpoint)
        .unwrap();
    let [local, remote] = fixture.clients();
    let provider = Provider {
        id: ProviderId::new(),
        revision: 0,
        preset: None,
        binding: None,
        consent: None,
        name: "Score fixture".into(),
        api: ModelApi::ChatCompletions,
        authentication: Authentication::ApiKey,
        endpoint: "http://127.0.0.1:9/v1".into(),
        options: None,
        oauth: None,
        enabled: true,
        default_model: "claude-sonnet-5-5".into(),
        request_max_retries: None,
        stream_max_retries: None,
        credential: None,
        models: vec![Model {
            id: "claude-sonnet-5-5".into(),
            display_name: None,
            context: 8192,
            output: 1024,
            compaction_tokens: None,
            capabilities: None,
            custom_efforts: false,
            vision: false,
            tools: true,
            reasoning: true,
            generates: vec![],
            efforts: vec![Effort::XHigh],
            default_effort: Effort::XHigh,
        }],
    };
    let provider_id = provider.id;
    local
        .execute(local.prepare(Command::PutProvider {
            provider,
            expected_revision: 0,
        }))
        .await
        .unwrap();
    let read = Command::ReadModelScores(vec![
        catalog::ModelRef {
            provider: provider_id,
            model: "claude-sonnet-5-5".into(),
        },
        catalog::ModelRef {
            provider: provider_id,
            model: "claude-sonnet-5".into(),
        },
        catalog::ModelRef {
            provider: ProviderId::new(),
            model: "claude-sonnet-5-5".into(),
        },
    ]);
    for client in [&local, &remote] {
        let mut updates = client.subscribe().await.unwrap();
        let mut projection = Projection::new(client.target(), 1);
        client.recover(&mut projection, 1).await.unwrap();
        let Output::CatalogStatus(status) = client
            .execute(client.prepare(Command::RefreshModelCatalog))
            .await
            .unwrap()
        else {
            panic!("catalog status expected")
        };
        assert_eq!(status.scores.models, 3);
        assert!(
            serde_json::to_value(&status.scores)
                .unwrap()
                .get("enabled")
                .is_none()
        );
        let admission = client.dispatch(client.prepare(read.clone())).await.unwrap();
        assert!(!admission.receipt.durable);
        let expected = admission.completion.await.unwrap().unwrap();
        assert_eq!(
            local.execute(local.prepare(read.clone())).await.unwrap(),
            expected
        );
        assert_eq!(
            remote.execute(remote.prepare(read.clone())).await.unwrap(),
            expected
        );
        let Output::ModelScores(scores) = expected else {
            panic!("model scores expected")
        };
        assert_eq!(scores.scores.len(), 1);
        assert_eq!(scores.revision, status.scores.revision);
        assert_eq!(scores.scores[0].provider, provider_id);
        assert_eq!(scores.scores[0].efforts[0].effort, Effort::XHigh);
        assert_eq!(scores.scores[0].efforts[0].overall, 81.95);
        tokio::time::timeout(Duration::from_secs(5), async {
            while projection.snapshot().unwrap().model_catalog != status {
                projection.apply(1, updates.next().await.unwrap()).unwrap();
            }
        })
        .await
        .unwrap();
    }
    fixture.close().await;
}
