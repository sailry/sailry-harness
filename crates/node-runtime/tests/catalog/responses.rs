use super::*;

#[tokio::test]
async fn pages_complete_models() {
    let values = vec!["\"".repeat(64); 32];
    let options = vec![json!({"type":"effort", "values":values}); 16];
    let models: serde_json::Map<String, Value> = (0..100)
        .map(|index| {
            let id = format!("model-{index:03}");
            (id.clone(), json!({"id":id,"reasoning_options":options}))
        })
        .collect();
    for remote in [false, true] {
        let catalog = json!({"fixture":{"models":models}});
        let server =
            Server::start(ModelApi::Anthropic, move |_| Reply::Json(catalog.clone())).await;
        let fixture = Fixture::new(remote, &server).await;
        refresh(&fixture.client).await;
        for lookup in [false, true] {
            let mut query = query();
            query.limit = 100;
            if lookup {
                query.provider = None;
                query.ids = (0..100).map(|index| format!("model-{index:03}")).collect();
            }
            let mut ids = Vec::new();
            loop {
                let page = page(&fixture.client, query.clone()).await;
                assert!(serde_json::to_vec(&page).unwrap().len() < 513 * 1024);
                assert!(!page.models.is_empty() && page.models.len() < 100);
                ids.extend(page.models.into_iter().map(|model| model.id));
                if page.next.is_none() {
                    break;
                }
                query.revision = Some(page.revision);
                query.after = page.next;
            }
            assert_eq!(
                ids,
                (0..100)
                    .map(|index| format!("model-{index:03}"))
                    .collect::<Vec<_>>()
            );
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn rejects_invalid_pages() {
    for remote in [false, true] {
        let mut invalid = vec![json!([]), json!({}), json!({"provider":{"models":[]}})];
        for model in [
            json!({}),
            json!({"id":""}),
            json!({"id":"ok","name":false}),
            json!({"id":"ok","limit":[]}),
            json!({"id":"ok","limit":{"context":-1}}),
            json!({"id":"ok","limit":{"output":4294967296u64}}),
            json!({"id":"ok","tool_call":"yes"}),
            json!({"id":"ok","modalities":{"input":[2]}}),
            json!({"id":"ok","reasoning_options":[{"type":"budget_tokens","min":-2}]}),
            json!({"id":"ok","reasoning_options":[{"type":"budget_tokens","min":8,"max":4}]}),
            json!({"id":"ok","reasoning_options":[{"type":"effort","values":["bad\nvalue"]}]}),
        ] {
            invalid.push(json!({"provider":{"models":{"model":model}}}));
        }
        invalid.push(json!({"provider":{"models":{"a":{"id":"same"},"b":{"id":"same"}}}}));
        let count = AtomicUsize::new(0);
        let total = invalid.len();
        let server = Server::start(ModelApi::Anthropic, move |_| {
            Reply::Json(
                invalid
                    .get(count.fetch_add(1, Ordering::SeqCst))
                    .cloned()
                    .unwrap_or_else(data),
            )
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        for _ in 0..total {
            assert!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::RefreshModelCatalog))
                    .await
                    .is_err()
            );
            assert_eq!(status(&fixture.client).await.revision, 0);
        }
        refresh(&fixture.client).await;
        let mut invalid = vec![];
        for size in [0, 101] {
            let mut input = query();
            input.limit = size;
            invalid.push(input);
        }
        let mut after = query();
        after.after = Some("known".into());
        invalid.push(after);
        let mut provider = query();
        provider.provider = Some("bad\nprovider".into());
        invalid.push(provider);
        for input in invalid {
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ReadModelCatalog(input)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        let mut missing = query();
        missing.provider = Some("missing".into());
        assert!(page(&fixture.client, missing).await.models.is_empty());
        fixture.close().await;
    }
}

#[tokio::test]
async fn confines_downloads() {
    for remote in [false, true] {
        let target = Server::start(ModelApi::Anthropic, |_| Reply::Json(data())).await;
        let location = target.endpoint.clone();
        let count = AtomicUsize::new(0);
        let server = Server::start(ModelApi::Anthropic, move |_| Reply::Raw(match count.fetch_add(1, Ordering::SeqCst) {
            0 => format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
            1 => "HTTP/1.1 200 OK\r\nContent-Length: 33554433\r\nConnection: close\r\n\r\n".into(),
            2 => "HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx".into(),
            3 => format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2000001\r\n{}\r\n0\r\n\r\n", " ".repeat(32*1024*1024+1)),
            _ => "HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        })).await;
        let fixture = Fixture::new(remote, &server).await;
        for _ in 0..5 {
            let error = fixture
                .client
                .execute(fixture.client.prepare(Command::RefreshModelCatalog))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::Unavailable);
            assert!(!error.message.contains(&target.endpoint));
            assert_eq!(status(&fixture.client).await.revision, 0);
        }
        assert!(target.requests.lock().unwrap().is_empty());
        fixture.close().await;
    }
}
