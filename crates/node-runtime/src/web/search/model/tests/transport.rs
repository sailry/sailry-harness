use super::*;

fn combinations() -> [(Authentication, ModelApi); 9] {
    [
        (Authentication::ApiKey, ModelApi::Responses),
        (Authentication::ChatGpt, ModelApi::Responses),
        (Authentication::ChatGptOAuth, ModelApi::Responses),
        (Authentication::ApiKey, ModelApi::Anthropic),
        (Authentication::ClaudeOAuth, ModelApi::Anthropic),
        (Authentication::ApiKey, ModelApi::Gemini),
        (Authentication::Plugin, ModelApi::Responses),
        (Authentication::Plugin, ModelApi::Anthropic),
        (Authentication::Plugin, ModelApi::Gemini),
    ]
}

#[tokio::test]
async fn sends_native_requests_across_logins() {
    let client = client();
    for (authentication, api) in combinations() {
        let streaming = authentication == Authentication::ChatGptOAuth;
        let value = response(api);
        let body = if streaming {
            format!(
                "event: response.completed\r\ndata: {}\r\n\r\n",
                json!({"type":"response.completed","response":value})
            )
        } else {
            value.to_string()
        };
        let server = fixture::Server::start(
            200,
            if streaming {
                "text/event-stream"
            } else {
                "application/json"
            },
            &body,
        )
        .await;
        let mut provider = auth::provider(authentication, api);
        provider.endpoint = server.endpoint.clone();
        let prepared = prepare(
            &provider,
            &auth::credentials(authentication),
            "fixture-model",
            auth::SESSION.parse().unwrap(),
            auth::REQUEST.parse().unwrap(),
            &input(),
            true,
        )
        .unwrap();
        assert_eq!(prepared.body, expected(api, authentication));
        let parsed = execute(
            &client,
            prepared,
            &Run::new(&provider, "fixture-model", None, CancellationToken::new()),
        )
        .await
        .unwrap();
        assert_eq!(parsed.summary.as_deref(), Some("Generated answer"));
        assert_eq!(parsed.results[0].url, "https://example.com/source");
        let captured = server.captured().await;
        let path = match api {
            ModelApi::Responses => "/responses",
            ModelApi::Anthropic => "/v1/messages",
            ModelApi::Gemini => "/v1beta/models/fixture-model:generateContent",
            _ => unreachable!(),
        };
        assert_eq!(captured.route, format!("POST {path} HTTP/1.1"));
        assert_eq!(captured.body, expected(api, authentication));
        assert_headers(&captured, authentication, api);
    }
}

#[tokio::test]
async fn keeps_rejected_credentials_out_of_faults() {
    let client = client();
    for (authentication, api) in combinations() {
        for status in [400, 401, 403, 429, 503] {
            let response = json!({"error":{
                "code":"invalid_request_error",
                "message":"fixed-access fixed-refresh private-account https://secret.test?key=fixed-access"
            }});
            let server =
                fixture::Server::start(status, "application/json", &response.to_string()).await;
            let mut provider = auth::provider(authentication, api);
            provider.endpoint = server.endpoint.clone();
            let prepared = prepare(
                &provider,
                &auth::credentials(authentication),
                "fixture-model",
                auth::SESSION.parse().unwrap(),
                auth::REQUEST.parse().unwrap(),
                &input(),
                true,
            )
            .unwrap();
            let error = execute(
                &client,
                prepared,
                &Run::new(&provider, "fixture-model", None, CancellationToken::new()),
            )
            .await
            .unwrap_err();
            let serialized = serde_json::to_string(&error.fault).unwrap();
            let debug = format!("{error:?}");
            for secret in [
                "fixed-access",
                "fixed-refresh",
                "private-account",
                "secret.test",
            ] {
                assert!(!serialized.contains(secret));
                assert!(!debug.contains(secret));
            }
            let _ = server.captured().await;
        }
    }
}

#[tokio::test]
async fn distinguishes_search_rejection_from_authentication() {
    let client = client();
    for (status, message, unsupported) in [
        (400, "This model does not support web_search", true),
        (403, "web_search is not enabled for this plan", true),
        (400, "max_tokens is invalid", false),
        (401, "web_search key is not allowed", false),
    ] {
        let server = fixture::Server::start(
            status,
            "application/json",
            &json!({"error":{"message":message}}).to_string(),
        )
        .await;
        let mut provider = auth::provider(Authentication::ApiKey, ModelApi::Responses);
        provider.endpoint = server.endpoint.clone();
        let prepared = prepare(
            &provider,
            &auth::credentials(Authentication::ApiKey),
            "fixture-model",
            auth::SESSION.parse().unwrap(),
            auth::REQUEST.parse().unwrap(),
            &input(),
            true,
        )
        .unwrap();
        let error = execute(
            &client,
            prepared,
            &Run::new(&provider, "fixture-model", None, CancellationToken::new()),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.fault.details == Some(FaultDetails::SearchUnsupported),
            unsupported
        );
        if status == 401 {
            assert_eq!(
                error.fault.details,
                Some(FaultDetails::ModelRequest {
                    reason: ModelFailure::Unauthorized
                })
            );
        }
        let _ = server.captured().await;
    }
}

#[tokio::test]
async fn rejects_credential_echoes_in_successful_replies() {
    let client = client();
    for (authentication, api) in combinations() {
        let mut value = response(api);
        match api {
            ModelApi::Responses => {
                value["output"][2]["content"][0]["text"] = json!("Echo: fixed-access")
            }
            ModelApi::Anthropic => value["content"][2]["text"] = json!("Echo: fixed-access"),
            ModelApi::Gemini => {
                value["candidates"][0]["content"]["parts"][1]["text"] = json!("Echo: fixed-access")
            }
            _ => unreachable!(),
        }
        let server = fixture::Server::start(200, "application/json", &value.to_string()).await;
        let mut provider = auth::provider(authentication, api);
        provider.endpoint = server.endpoint.clone();
        let prepared = prepare(
            &provider,
            &auth::credentials(authentication),
            "fixture-model",
            auth::SESSION.parse().unwrap(),
            auth::REQUEST.parse().unwrap(),
            &input(),
            true,
        )
        .unwrap();
        let error = execute(
            &client,
            prepared,
            &Run::new(&provider, "fixture-model", None, CancellationToken::new()),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.fault.message,
            "model search response contains protected data"
        );
        assert!(!format!("{error:?}").contains("fixed-access"));
        let _ = server.captured().await;
    }
}
