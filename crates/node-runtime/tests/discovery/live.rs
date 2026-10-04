use super::*;

#[tokio::test]
#[ignore = "requires SAILRY_ACCEPTANCE_PROVIDER_ENDPOINT, SAILRY_ACCEPTANCE_PROVIDER_KEY and SAILRY_ACCEPTANCE_PROVIDER_API"]
async fn model_catalog() {
    let endpoint = std::env::var("SAILRY_ACCEPTANCE_PROVIDER_ENDPOINT").unwrap();
    let secret = Secret::new(std::env::var("SAILRY_ACCEPTANCE_PROVIDER_KEY").unwrap());
    let api: ModelApi = serde_json::from_value(json!(
        std::env::var("SAILRY_ACCEPTANCE_PROVIDER_API").unwrap()
    ))
    .unwrap();
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let result = fixture
            .client
            .execute(fixture.client.prepare(discover(discovery::Draft {
                provider: ProviderId::new(),
                api,
                endpoint: endpoint.clone(),
                credential: None,
                secret: Some(secret.clone()),
            })))
            .await;
        fixture.close().await;
        let Output::DiscoveredModels(discovery::Catalog { endpoint, models }) = result.unwrap()
        else {
            panic!("models expected")
        };
        assert!(!models.is_empty());
        println!(
            "{api:?} discovery remote={remote}: {} models, endpoint={endpoint}",
            models.len()
        );
    }
}
