use super::*;

fn status(code: u16) -> Reply {
    Reply::Raw(format!(
        "HTTP/1.1 {code} Fixture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    ))
}

#[tokio::test]
async fn resolves_standard_paths() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        for (api, input, resolved, requests) in [
            (ModelApi::Responses, "", "", 1),
            (ModelApi::Responses, "", "/v1", 2),
            (ModelApi::ChatCompletions, "/", "/v1", 2),
            (ModelApi::DeepSeek, "", "/v1", 2),
            (ModelApi::Responses, "/gateway/", "/gateway/v1", 2),
            (ModelApi::Responses, "/gateway/v2/", "/gateway/v2", 1),
            (ModelApi::Gemini, "", "", 1),
            (ModelApi::Gemini, "", "/v1beta", 2),
            (ModelApi::Gemini, "", "/v1", 3),
            (ModelApi::Gemini, "/gateway", "/gateway/v1beta", 2),
            (ModelApi::Gemini, "/v1/", "/v1", 1),
        ] {
            let expected = format!("{resolved}/models");
            let server = Server::start(ModelApi::Anthropic, move |path| {
                if path.split('?').next() == Some(expected.as_str()) {
                    Reply::Json(catalog(api, true))
                } else if api == ModelApi::Gemini {
                    Reply::Raw("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: 13\r\nConnection: close\r\n\r\n<html></html>".into())
                } else {
                    status(404)
                }
            })
            .await;
            let mut draft = source(api, &server);
            draft.endpoint.push_str(input);
            let Output::DiscoveredModels(result) = execute(&fixture.client, discover(draft)).await
            else {
                panic!("catalog expected")
            };
            assert_eq!(result.endpoint, format!("{}{resolved}", server.endpoint));
            assert_eq!(result.models.len(), 2);
            let actual = server.requests.lock().unwrap();
            assert_eq!(actual.len(), requests);
            for request in actual.iter() {
                let header = if api == ModelApi::Gemini {
                    "x-goog-api-key"
                } else {
                    "authorization"
                };
                assert_eq!(
                    request.headers[header],
                    if api == ModelApi::Gemini {
                        KEY.to_owned()
                    } else {
                        format!("Bearer {KEY}")
                    }
                );
            }
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn handles_mixed_gateway_metadata() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let server = Server::start(ModelApi::Anthropic, |path| {
            if path.starts_with("/v1beta/models?") {
                Reply::Json(json!({"models":[
                    {"name":"known", "inputTokenLimit":null, "outputTokenLimit":null,
                     "supportedGenerationMethods":null},
                    {"name":"models/unknown"},
                    {"name":"models/embedding", "supportedGenerationMethods":["embedContent"]}
                ], "nextPageToken":null}))
            } else {
                Reply::Json(json!({"object":"list", "data":[{"id":"other-protocol"}]}))
            }
        })
        .await;
        let Output::DiscoveredModels(catalog) =
            execute(&fixture.client, discover(source(ModelApi::Gemini, &server))).await
        else {
            panic!("catalog expected")
        };
        assert_eq!(catalog.endpoint, format!("{}/v1beta", server.endpoint));
        assert_eq!(
            catalog.models,
            ["known", "unknown"].map(|id| discovery::Model {
                id: id.into(),
                display_name: None,
                context: None,
                output: None,
                capabilities: None,
            })
        );
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0].path, "/models?pageSize=100");
            assert_eq!(requests[1].path, "/v1beta/models?pageSize=100");
            for request in requests.iter() {
                assert_eq!(request.headers["x-goog-api-key"], KEY);
                assert!(!request.path.contains(KEY));
            }
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn stops_on_authentication_and_server_errors() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        for code in [401, 403, 429, 500, 503, 307] {
            let server = Server::start(ModelApi::Anthropic, move |_| status(code)).await;
            let error = fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(discover(source(ModelApi::Gemini, &server))),
                )
                .await
                .unwrap_err();
            assert_eq!(
                error.code,
                if matches!(code, 401 | 403) {
                    ErrorCode::NotConfigured
                } else {
                    ErrorCode::Unavailable
                }
            );
            assert_eq!(server.requests.lock().unwrap().len(), 1);
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn does_not_switch_paths_midway() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        for foreign_catalog in [false, true] {
            let server = Server::start(ModelApi::Anthropic, move |path| {
                if !path.contains("pageToken=") {
                    Reply::Json(catalog(ModelApi::Gemini, false))
                } else if foreign_catalog {
                    Reply::Json(json!({"object":"list", "data":[]}))
                } else {
                    status(404)
                }
            })
            .await;
            assert!(
                fixture
                    .client
                    .execute(
                        fixture
                            .client
                            .prepare(discover(source(ModelApi::Gemini, &server)))
                    )
                    .await
                    .is_err()
            );
            assert_eq!(server.requests.lock().unwrap().len(), 2);
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn bounds_unsuccessful_probes() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        for (api, count) in [(ModelApi::Responses, 2), (ModelApi::Gemini, 3)] {
            let server = Server::start(ModelApi::Anthropic, |_| status(404)).await;
            assert!(
                fixture
                    .client
                    .execute(fixture.client.prepare(discover(source(api, &server))))
                    .await
                    .is_err()
            );
            assert_eq!(server.requests.lock().unwrap().len(), count);
        }
        fixture.close().await;
    }
}
