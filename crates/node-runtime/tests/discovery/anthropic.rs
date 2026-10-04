use super::*;

#[tokio::test]
async fn accepts_root_and_versioned_endpoints() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        for prefix in ["", "/gateway", "/gateway-v1"] {
            for suffix in ["", "/", "/v1", "/v1/"] {
                let expected = format!("{prefix}/v1/models?limit=100");
                let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                    assert_eq!(request.method, "GET");
                    assert_eq!(request.path, expected);
                    assert_eq!(request.headers["x-api-key"], KEY);
                    assert_eq!(request.headers["anthropic-version"], "2023-06-01");
                    Reply::Json(json!({"data":[{"id":"claude-fixture"}], "has_more":false}))
                })
                .await;
                let mut input = source(ModelApi::Anthropic, &server);
                input.endpoint = format!("{}{prefix}{suffix}", server.endpoint);
                let Output::DiscoveredModels(discovery::Catalog { models, .. }) =
                    execute(&fixture.client, discover(input)).await
                else {
                    panic!("models expected")
                };
                assert_eq!(models.len(), 1);
                assert_eq!(models[0].id, "claude-fixture");
                assert_eq!(server.requests.lock().unwrap().len(), 1);
            }
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn reads_compatible_catalogs() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        for paginated in [false, true] {
            let server = Server::start(ModelApi::Anthropic, move |path| {
                let second = path.contains("after_id=claude-first");
                let mut value = json!({
                    "object":"list",
                    "data":[{
                        "id":if second { "claude-second" } else { "claude-first" },
                        "type":"model", "display_name":"Claude fixture",
                        "created_at":"2026-01-01T00:00:00Z"
                    }]
                });
                if paginated {
                    value["has_more"] = json!(!second);
                    value["last_id"] = value["data"][0]["id"].clone();
                }
                Reply::Json(value)
            })
            .await;
            let mut input = source(ModelApi::Anthropic, &server);
            input.endpoint.push_str("/v1");
            let Output::DiscoveredModels(discovery::Catalog { models, .. }) =
                execute(&fixture.client, discover(input)).await
            else {
                panic!("models expected")
            };
            assert_eq!(models.len(), if paginated { 2 } else { 1 });
            assert_eq!(models[0].id, "claude-first");
            assert_eq!(server.requests.lock().unwrap().len(), models.len());
        }
        fixture.close().await;
    }
}
