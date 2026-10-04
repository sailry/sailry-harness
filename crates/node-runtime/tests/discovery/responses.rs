use super::*;

#[tokio::test]
async fn rejects_unsafe_responses() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let target = Server::start(ModelApi::Responses, |_| Reply::Json(json!({"data":[]}))).await;
        let redirect = format!(
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: {}/models\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            target.endpoint
        );
        for (response, code) in [
            (
                format!(
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{KEY}",
                    KEY.len()
                ),
                ErrorCode::NotConfigured,
            ),
            (
                format!(
                    "HTTP/1.1 503 Unavailable\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{KEY}",
                    KEY.len()
                ),
                ErrorCode::Unavailable,
            ),
            (redirect, ErrorCode::Unavailable),
            (
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{KEY}",
                    KEY.len()
                ),
                ErrorCode::Unavailable,
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 2097153\r\nConnection: close\r\n\r\nx".into(),
                ErrorCode::Unavailable,
            ),
            (
                format!(
                    "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{}",
                    "x".repeat(2097153)
                ),
                ErrorCode::Unavailable,
            ),
        ] {
            let server =
                Server::start(ModelApi::Responses, move |_| Reply::Raw(response.clone())).await;
            let error = fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(discover(source(ModelApi::Responses, &server))),
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, code);
            assert!(!error.message.contains(KEY));
            assert_eq!(server.requests.lock().unwrap().len(), 1);
        }
        assert!(target.requests.lock().unwrap().is_empty());
        fixture.close().await;
    }
}

#[tokio::test]
async fn rejects_partial_or_conflicting_lists() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        for (api, value) in [
            (
                ModelApi::Responses,
                json!({"data":[{"id":"duplicate", "context_length":100}, {"id":"duplicate", "context_length":200}]}),
            ),
            (
                ModelApi::Responses,
                json!({"data":[{"id":"broken", "context_length":-1}]}),
            ),
            (
                ModelApi::Responses,
                json!({"data":[{"id":"conflict", "context_length":100, "context_window":200}]}),
            ),
            (
                ModelApi::Responses,
                json!({"data":[{"id":"broken", "max_output_tokens":"100"}]}),
            ),
            (
                ModelApi::Responses,
                json!({"data":[{"id":"bad\nidentifier"}]}),
            ),
            (
                ModelApi::Responses,
                json!({"data":(0..1001).map(|id| json!({"id":format!("model-{id}")})).collect::<Vec<_>>()}),
            ),
            (ModelApi::Anthropic, json!({"data":[]})),
            (ModelApi::Anthropic, json!({"data":[], "has_more":true})),
            (
                ModelApi::Anthropic,
                json!({"object":"list", "data":[], "has_more":null}),
            ),
            (
                ModelApi::Anthropic,
                json!({"object":"list", "data":[], "has_more":"false"}),
            ),
            (
                ModelApi::Anthropic,
                json!({"object":"list", "data":[], "has_more":true}),
            ),
            (
                ModelApi::Anthropic,
                json!({"data":[], "has_more":true, "last_id":"repeated"}),
            ),
            (
                ModelApi::Gemini,
                json!({"models":[], "nextPageToken":"repeated"}),
            ),
        ] {
            let server = Server::start(api, move |_| Reply::Json(value.clone())).await;
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(discover(source(api, &server))))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Unavailable
            );
        }
        fixture.close().await;
    }
}
