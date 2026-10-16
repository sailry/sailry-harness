use super::*;

const ANSWER: &str = "Authorized search result";
const SOURCE: &str = "https://example.com/source";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selects_native_search_from_authorized_contract() {
    for remote in [false, true] {
        for metadata in [None, Some(true), Some(false)] {
            let native = metadata != Some(false);
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                reply(request, metadata)
            })
            .await;
            let fixture = Fixture::empty(remote, &server).await;
            let provider = provider(&fixture.client, Authentication::ChatGptOAuth).await;
            assert!(provider.models.is_empty());
            let (_, attempt) = begin(&fixture.client, &provider).await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            let connected = terminal(&mut stream).await;
            assert_eq!(connected.state, login::State::Connected);
            assert_eq!(connected.model_error, None);
            drop(stream);
            let provider = saved(&fixture.client, provider.id).await;
            assert_eq!(provider.models[0].id, "fixture");
            assert_eq!(provider.models[0].web_search, native);
            assert_eq!(
                provider.models[0].capabilities.as_ref().unwrap().web_search,
                metadata
            );
            let session = super::super::execution::session(&fixture, &provider).await;
            assert_eq!(session.config.permission, Permission::Ask);
            let (_, turn) = super::super::execution::submit(&fixture.client, &session).await;
            let page =
                super::super::execution::finished(&fixture.client, &turn, Status::Completed).await;
            assert_eq!(answer(&page, turn.id), ANSWER);
            let entries: Vec<_> = page
                .entries
                .iter()
                .filter(|entry| entry.turn == turn.id)
                .collect();
            assert_eq!(
                entries.iter().any(|entry| entry.parts.iter().any(|part| {
                    matches!(part, Part::ToolCall { name, .. } if name == "web_search")
                })),
                native
            );
            assert_eq!(
                entries.iter().any(|entry| entry
                    .citations
                    .iter()
                    .any(|citation| citation.uri == SOURCE)),
                native
            );
            assert!(page.questions.is_empty());
            assert_eq!(
                server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| request.path == "/responses")
                    .count(),
                1
            );
            fixture.close().await;
        }
    }
}

fn reply(request: &discovery_support::Request, metadata: Option<bool>) -> Reply {
    if request.path.starts_with("/models") {
        let Reply::Json(mut catalog) = server::standard(request, 3600) else {
            panic!("model catalog expected")
        };
        if let Some(supported) = metadata {
            catalog["models"][0]["capabilities"] =
                json!({"server_tools":{"web_search":{"supported":supported}}});
        }
        return Reply::Json(catalog);
    }
    if request.path != "/responses" {
        return server::standard(request, 3600);
    }
    let native = metadata != Some(false);
    let body = server::input(
        request,
        &Options::defaults(Authentication::ChatGptOAuth).unwrap(),
        ACCESS,
    );
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(
        tools
            .iter()
            .filter(|tool| tool["type"] == "web_search")
            .count(),
        usize::from(native)
    );
    let digest = blake3::hash(b"web-search").to_hex();
    let fallback = format!("plugin_{}_web_search", &digest[..16]);
    assert_eq!(
        tools
            .iter()
            .filter(|tool| tool["type"] == "function" && tool["name"] == fallback)
            .count(),
        usize::from(!native)
    );
    let mut output = Vec::new();
    if native {
        output.push(json!({"type":"web_search_call", "id":"authorized-search",
            "status":"completed", "action":{"type":"search", "query":"fixture source"}}));
    }
    let mut message = server::message(ANSWER);
    if native {
        message["content"][0]["annotations"] = json!([{"type":"url_citation",
            "start_index":0, "end_index":ANSWER.chars().count(), "title":"Fixture source", "url":SOURCE}]);
    }
    output.push(message);
    server::stream(
        vec![server::delta(ANSWER), server::completed(json!(output))],
        true,
    )
}
