use super::*;

const ANSWER: &str = "Authorized search result";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offers_configured_search() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, reply).await;
        let fixture = Fixture::packages(remote, &server, &["web-search"]).await;
        let Output::SearchSettings(mut settings) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadSearchSettings))
            .await
            .unwrap()
        else {
            panic!("search settings expected")
        };
        settings.driver = Some(sailry_protocol::search::Selection::Driver(
            sailry_protocol::search::Driver::Exa,
        ));
        fixture
            .client
            .execute(fixture.client.prepare(Command::SaveSearchSettings(
                sailry_protocol::search::Update {
                    settings,
                    credentials: Default::default(),
                },
            )))
            .await
            .unwrap();
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
        assert!(!entries.iter().any(|entry| {
            entry
                .parts
                .iter()
                .any(|part| matches!(part, Part::ToolCall { name, .. } if name == "web_search"))
        }));
        assert!(entries.iter().all(|entry| entry.citations.is_empty()));
        assert!(page.questions.is_empty());
        assert_eq!(saved(&fixture.client, provider.id).await, provider);
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

fn reply(request: &discovery_support::Request) -> Reply {
    if request.path != "/responses" {
        return server::standard(request, 3600);
    }
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
        0
    );
    let digest = blake3::hash(b"web-search").to_hex();
    let fallback = format!("plugin_{}_web_search", &digest[..16]);
    assert_eq!(
        tools
            .iter()
            .filter(|tool| tool["type"] == "function" && tool["name"] == fallback)
            .count(),
        1
    );
    server::stream(
        vec![
            server::delta(ANSWER),
            server::completed(json!([server::message(ANSWER)])),
        ],
        true,
    )
}
