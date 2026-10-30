use super::*;
use serde_json::json;

fn response(provider: ProviderId, input: i32) -> canonical::Record {
    let mut search = event(START + 1000, input);
    search.provenance.author = "system".into();
    search.message_mut().unwrap().content.clear();
    search.message_mut().unwrap().part_count = 0;
    search.message_mut().unwrap().model = Some("search-model".into());
    search.metadata.execution = Some(canonical::Execution {
        purpose: Some(canonical::Purpose::Search),
        source: Some(canonical::ModelSource {
            id: provider,
            name: "Search provider".into(),
            api: ModelApi::Anthropic,
            authentication: Authentication::ApiKey,
            model: "search-model".into(),
        }),
        ..Default::default()
    });
    search.metadata.runtime = Some(canonical::Runtime {
        user_visible: false,
        agent_visible: false,
        ..Default::default()
    });
    search
}

#[tokio::test]
async fn attributes_usage_to_the_selected_model() {
    let mut fixture = Fixture::new().await;
    let provider = ProviderId::new();
    fixture
        .database
        .connection
        .execute(
            "INSERT INTO model_catalog(provider,id,body) VALUES('anthropic','search-model',?1)",
            [serde_json::to_vec(&json!({
                "id":"search-model", "cost":{"input":1,"output":2,"cache_read":0.5}
            }))
            .unwrap()],
        )
        .await
        .unwrap();
    let search = response(provider, 30);
    let turn = fixture.append([event(START, 10), search]).await;
    let result = report(&fixture, &query()).await;
    assert_eq!(result.totals.responses, 2);
    assert_eq!(result.totals.tokens.as_ref().unwrap().input, 40);
    let call = result
        .requests
        .items
        .iter()
        .find(|call| call.provider == provider)
        .unwrap();
    assert_eq!(call.turn, turn);
    assert_eq!(call.session, fixture.session);
    assert_eq!(call.provider_name, "Search provider");
    assert_eq!(call.model, "search-model");
    assert_eq!(call.tokens.input, 30);
    assert_eq!(call.tokens.cached_input, 2);
    assert_eq!(call.usd_micros, Some(37));
    let statistics = super::super::super::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(statistics.responses, 2);
    assert_eq!(statistics.usage, result.totals.tokens);
    assert_eq!(
        statistics.cost.as_ref().unwrap().usd_micros,
        result.totals.cost.as_ref().unwrap().usd_micros
    );
    let mut selection = query();
    selection.providers = vec![provider];
    selection.models = vec!["search-model".into()];
    let filtered = report(&fixture, &selection).await;
    assert_eq!(filtered.totals.responses, 1);
    assert_eq!(filtered.totals.tokens.as_ref().unwrap().input, 30);
    assert_eq!(filtered.groups.len(), 1);
    assert_eq!(filtered.groups[0].name.as_deref(), Some("Search provider"));
    selection.models = vec!["fixture".into()];
    assert_eq!(report(&fixture, &selection).await.totals.responses, 0);
}

#[tokio::test]
async fn prices_cache_writes_separately() {
    let mut fixture = Fixture::new().await;
    fixture
        .database
        .connection
        .execute(
            "INSERT INTO model_catalog(provider,id,body) VALUES('anthropic','search-model',?1)",
            [serde_json::to_vec(&json!({
                "id":"search-model",
                "cost":{"input":2,"output":8,"cache_read":0.5,"cache_write":5}
            }))
            .unwrap()],
        )
        .await
        .unwrap();
    let mut event = response(ProviderId::new(), 100);
    let usage = event.message_mut().unwrap().usage.as_mut().unwrap();
    usage.cache_read = Some(20);
    usage.cache_write = Some(50);
    fixture.append([event]).await;
    let result = report(&fixture, &query()).await;
    assert_eq!(result.totals.responses, 1);
    assert_eq!(result.totals.tokens.as_ref().unwrap().input, 100);
    assert_eq!(result.requests.items[0].usd_micros, Some(352));
    let cost = result.totals.cost.as_ref().unwrap();
    assert_eq!(cost.usd_micros, 352);
    assert_eq!(
        cost.breakdown,
        Some(sailry_protocol::usage::CostBreakdown {
            input: 60,
            output: 32,
            cache_read: 10,
            cache_write: 250,
        })
    );
    let statistics = super::super::super::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(statistics.usage, result.totals.tokens);
    assert_eq!(statistics.cost, result.totals.cost);
}
