use super::*;
use sailry_protocol::{Fault, FaultDetails, ModelFailure};

#[path = "../support/search/model.rs"]
mod native;

#[tokio::test]
async fn rejects_copilot_before_authorization() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let provider = serde_json::from_value(serde_json::json!({
            "id":sailry_protocol::ProviderId::new(), "revision":0,
            "name":"Unsupported search account", "api":"responses", "authentication":"copilot",
            "endpoint":"https://api.githubcopilot.com", "enabled":true,
            "default_model":native::MODEL, "credential":null,
            "models":[{"id":native::MODEL,"context":8192,"output":512,"tools":true}]
        }))
        .unwrap();
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::PutProvider {
                provider,
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        native::select(&client, &provider, false).await;
        for command in [
            Command::SearchWeb(search::Request {
                query: native::QUERY.into(),
                ..Default::default()
            }),
            Command::TestSearchDriver {
                driver: Driver::Model,
            },
        ] {
            let error = client.execute(client.prepare(command)).await.unwrap_err();
            assert_eq!(error.details, Some(FaultDetails::SearchUnsupported));
        }
        let Output::PluginModels(catalog) = client
            .execute(client.prepare(Command::ListPluginModels))
            .await
            .unwrap()
        else {
            panic!("model catalog expected")
        };
        let choice = catalog
            .models
            .iter()
            .find(|choice| choice.id == format!("{}/{}", provider.id, native::MODEL))
            .unwrap();
        assert_eq!(
            choice.authentication,
            sailry_protocol::Authentication::Copilot
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

async fn page(client: &Client, query: &str) -> Result<search::Page, Fault> {
    match client
        .execute(client.prepare(Command::SearchWeb(search::Request {
            query: query.into(),
            limit: 1,
            ..Default::default()
        })))
        .await?
    {
        Output::SearchPage(page) => Ok(page),
        _ => panic!("search page expected"),
    }
}

#[tokio::test]
async fn returns_citations_and_checks_the_service() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let http = native::http::Server::start().await;
        let source = native::origin::Server::start().await;
        node.set_web_fixture(native::origin::HOST, source.address);
        let url = format!("{}/html", source.origin);
        http.respond(
            "/v1/responses",
            vec![
                native::streamed(&url),
                native::response(&url),
                native::response(&url),
            ],
        );
        let provider = native::provider(&client, &format!("{}/v1", http.endpoint)).await;
        let settings = native::select(&client, &provider, true).await;
        assert!(
            !serde_json::to_string(&settings)
                .unwrap()
                .contains(native::KEY)
        );
        let first = page(&client, native::QUERY).await.unwrap();
        native::page(&first, &url);
        assert_eq!(page(&client, native::QUERY).await.unwrap(), first);
        assert_eq!(http.count("/v1/responses"), 1);
        for _ in 0..2 {
            let Output::SearchPage(tested) = client
                .execute(client.prepare(Command::TestSearchDriver {
                    driver: Driver::Model,
                }))
                .await
                .unwrap()
            else {
                panic!("search test page expected")
            };
            native::page(&tested, &url);
        }
        assert_eq!(http.count("/v1/responses"), 3);
        {
            let requests = http.requests.lock().unwrap();
            native::request(&requests[0].0, &requests[0].1, native::QUERY);
            for (header, body) in &requests[1..] {
                native::request(header, body, "Sailry");
            }
        }
        {
            let requests = source.requests.lock().unwrap();
            assert!(!requests.is_empty());
            for header in requests.iter() {
                let header = header.to_ascii_lowercase();
                assert!(!header.contains("authorization:"));
                assert!(!header.contains("x-api-key:"));
                assert!(!header.contains("cookie:"));
            }
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn keeps_manual_search_explicit() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let http = native::http::Server::start().await;
        let fallback = native::http::Server::start().await;
        node.set_search_fixture(Driver::Exa, &fallback.url(Driver::Exa));
        let unsupported = || {
            native::http::Reply::Json(serde_json::json!({"error":{
                "code":"unsupported_tool", "message":format!("web_search is not supported: {}",native::KEY)
            }}))
        };
        http.respond(
            "/v1/responses",
            vec![
                unsupported(),
                unsupported(),
                native::http::Reply::Status(401, None),
            ],
        );
        let provider = native::provider(&client, &format!("{}/v1", http.endpoint)).await;
        native::select(&client, &provider, true).await;
        let error = page(&client, native::QUERY).await.unwrap_err();
        assert!(matches!(
            error.details.as_ref(),
            Some(FaultDetails::SearchUnsupported)
        ));
        assert!(!serde_json::to_string(&error).unwrap().contains(native::KEY));
        let error = client
            .execute(client.prepare(Command::TestSearchDriver {
                driver: Driver::Model,
            }))
            .await
            .unwrap_err();
        assert!(matches!(
            error.details.as_ref(),
            Some(FaultDetails::SearchUnsupported)
        ));
        let error = page(&client, "Check authorization").await.unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert!(matches!(
            error.details.as_ref(),
            Some(FaultDetails::ModelRequest {
                reason: ModelFailure::Unauthorized
            })
        ));
        assert!(!serde_json::to_string(&error).unwrap().contains(native::KEY));
        assert_eq!(http.count("/v1/responses"), 3);
        assert_eq!(fallback.count("/exa"), 0);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[path = "model/automatic.rs"]
mod automatic;
