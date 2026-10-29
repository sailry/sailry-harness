use super::*;
use crate::agent_support::Server as SearchServer;

#[tokio::test]
async fn omits_unavailable_search() {
    for remote in [false, true] {
        let server = SearchServer::start(false).await;
        let fixture = Fixture::packages(
            remote,
            ModelApi::ChatCompletions,
            &server.endpoint,
            &["web-search"],
        )
        .await;
        let Output::SearchSettings(mut settings) =
            execute(&fixture.client, Command::ReadSearchSettings).await
        else {
            panic!("search settings expected")
        };
        settings.driver = None;
        execute(
            &fixture.client,
            Command::SaveSearchSettings(sailry_protocol::search::Update {
                settings,
                credentials: Default::default(),
            }),
        )
        .await;
        let turn = fixture.submit("Search is required").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let run = page.runs.last().unwrap();
        assert_eq!(run.status, Status::Completed);
        assert!(run.error.is_none());
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert!(
            !requests[0]["tools"].as_array().unwrap().iter().any(|tool| {
                tool["type"] == "web_search" || tool["function"]["name"] == "web_search"
            })
        );
        assert!(page.entries.iter().all(|entry| entry.citations.is_empty()));
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
