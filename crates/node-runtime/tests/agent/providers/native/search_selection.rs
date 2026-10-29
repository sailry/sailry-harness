use super::*;
use sailry_protocol::search::{Selection, Update};

#[tokio::test]
async fn selects_configured_tool() {
    for remote in [false, true] {
        let server = crate::agent_support::Server::start(false).await;
        let mut fixture = Fixture::packages(
            remote,
            ModelApi::Responses,
            &server.endpoint,
            &["web-search"],
        )
        .await;
        for (metadata, summary, enabled) in [
            (Some(true), false, true),
            (Some(false), true, true),
            (None, false, true),
            (Some(true), true, true),
            (Some(false), false, false),
            (Some(true), false, false),
        ] {
            fixture.provider.models[0].web_search = summary;
            fixture.provider.models[0]
                .capabilities
                .get_or_insert_with(Default::default)
                .web_search = metadata;
            let Output::Provider(provider) = execute(
                &fixture.client,
                Command::PutProvider {
                    expected_revision: fixture.provider.revision,
                    provider: fixture.provider.clone(),
                },
            )
            .await
            else {
                panic!("provider expected");
            };
            fixture.provider = provider;
            let Output::SearchSettings(mut settings) =
                execute(&fixture.client, Command::ReadSearchSettings).await
            else {
                panic!("search settings expected");
            };
            if !enabled {
                settings.driver = None;
            } else {
                settings.driver = Some(Selection::Driver(sailry_protocol::search::Driver::Exa));
            }
            execute(
                &fixture.client,
                Command::SaveSearchSettings(Update {
                    settings,
                    credentials: Default::default(),
                }),
            )
            .await;
            let turn = fixture
                .submit("Answer using the available search capability")
                .await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            let requests = server.requests.lock().unwrap();
            let tools = requests.last().unwrap()["tools"].as_array().unwrap();
            assert_eq!(
                tools
                    .iter()
                    .filter(|tool| tool["type"] == "web_search")
                    .count(),
                0
            );
            assert_eq!(
                tools
                    .iter()
                    .filter(|tool| tool["type"] == "function"
                        && tool["name"] == plugin_tool("web-search", "web_search"))
                    .count(),
                usize::from(enabled)
            );
            assert_eq!(
                usize::from(enabled),
                tools
                    .iter()
                    .filter(|tool| {
                        tool["type"] == "web_search"
                            || tool["type"] == "function"
                                && tool["name"] == plugin_tool("web-search", "web_search")
                    })
                    .count()
            );
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
