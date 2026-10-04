use super::*;

#[tokio::test]
async fn matches_exact_ids_across_providers() {
    let data = json!({
        "first":{"models":{
            "known":{"id":"known","limit":{"context":8192,"output":1024},"tool_call":true,"reasoning":true,
                "modalities":{"input":["text","image"]}},
            "disjoint":{"id":"disjoint","modalities":{"input":["image"]}},
            "unknown":{"id":"unknown"}}},
        "second":{"models":{
            "known":{"id":"known","limit":{"context":4096},"tool_call":false},
            "disjoint":{"id":"disjoint","modalities":{"input":["text"]}}}},
        "third":{"models":{
            "known":{"id":"known","limit":{"output":512},"tool_call":true},
            "disjoint":{"id":"disjoint","modalities":{"input":["image"]}}}}
    });
    for remote in [false, true] {
        let data = data.clone();
        let server = Server::start(ModelApi::Anthropic, move |_| Reply::Json(data.clone())).await;
        let fixture = Fixture::new(remote, &server).await;
        refresh(&fixture.client).await;
        let mut query = query();
        query.provider = None;
        query.ids = vec![
            "known".into(),
            "disjoint".into(),
            "missing".into(),
            "known".into(),
            "unknown".into(),
        ];
        let mut result = Vec::new();
        loop {
            let page = page(&fixture.client, query.clone()).await;
            result.extend(page.models);
            if page.next.is_none() {
                break;
            }
            query.revision = Some(page.revision);
            query.after = page.next;
        }
        assert_eq!(
            result
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["disjoint", "known", "unknown"]
        );
        assert!(result[0].inputs.is_empty());
        let known = &result[1];
        assert_eq!((known.context, known.output), (Some(4096), Some(512)));
        assert_eq!(known.tools, Some(false));
        assert_eq!(known.reasoning, Some(true));
        assert_eq!(known.inputs, ["text", "image"]);
        assert_eq!(result[2].context, None);
        query.after = None;
        query.provider = Some("first".into());
        query.ids = vec!["known".into()];
        assert_eq!(
            page(&fixture.client, query).await.models[0].context,
            Some(8192)
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn rejects_unbounded_lookups() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::Anthropic, |_| Reply::Json(data())).await;
        let fixture = Fixture::new(remote, &server).await;
        refresh(&fixture.client).await;
        for ids in [vec![], vec!["known".into(); 101], vec!["bad\nid".into()]] {
            let mut query = query();
            query.provider = None;
            query.ids = ids;
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ReadModelCatalog(query)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        fixture.close().await;
    }
}
