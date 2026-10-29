use sailry_client::{Apply, Client, Projection};
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{
    Command, ErrorCode, Output, RequestId, Secret,
    plugin::settings::SecretUpdate,
    search::{self, Driver, Selection},
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
#[path = "search/domestic.rs"]
mod domestic;
#[path = "search/filters.rs"]
mod filters;
#[path = "search/rendering.rs"]
mod rendering;
#[path = "support/search.rs"]
mod server;
#[path = "search/tinyfish.rs"]
mod tinyfish;
use server::{Reply, Server};

async fn controller(node: &Node, path: &std::path::Path) -> (Link, Arc<dyn Transport>) {
    let controller = Link::controller(path.join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = controller.handle().remote(address);
    (controller, remote)
}

#[tokio::test]
#[ignore = "uses the public keyless Exa service"]
async fn searches_public_sources_without_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let (controller, remote) = controller(&node, directory.path()).await;
    for transport in [node.local(), remote] {
        let client = Client::new(transport);
        let settings = read(&client).await;
        assert!(!settings.providers[&Driver::Exa].key_configured);
        let Output::SearchPage(page) = client
            .execute(client.prepare(Command::SearchWeb(search::Request {
                query: "Rust language official documentation".into(),
                limit: 3,
                ..Default::default()
            })))
            .await
            .unwrap()
        else {
            panic!("search page expected")
        };
        assert_eq!(page.driver, Driver::Exa);
        assert!(page.untrusted);
        assert!(!page.results.is_empty());
        assert!(page.results.len() <= 3);
        for result in page.results {
            assert!(!result.title.is_empty());
            assert!(matches!(
                url::Url::parse(&result.url).unwrap().scheme(),
                "http" | "https"
            ));
        }
    }
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
}

#[tokio::test]
async fn searches_keyless_exa_without_initialization() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let http = Server::start().await;
        node.set_search_fixture(Driver::Exa, &format!("{}/modern-mcp", http.endpoint));

        let Output::SearchPage(page) = client
            .execute(client.prepare(Command::SearchWeb(search::Request {
                query: "Public source".into(),
                limit: 1,
                ..Default::default()
            })))
            .await
            .unwrap()
        else {
            panic!("search page expected")
        };
        assert_eq!(page.driver, Driver::Exa);
        assert_eq!(page.results.len(), 1);
        assert_eq!(page.results[0].url, "https://example.com/source");

        {
            let requests = http.requests.lock().unwrap();
            let methods: Vec<_> = requests
                .iter()
                .filter_map(|(_, body)| body["method"].as_str())
                .collect();
            assert_eq!(methods, ["server/discover", "tools/call"]);
            assert!(
                requests
                    .iter()
                    .all(|(_, body)| body["method"] != "initialize")
            );
        }

        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

async fn read(client: &Client) -> search::Settings {
    let Output::SearchSettings(settings) = client
        .execute(client.prepare(Command::ReadSearchSettings))
        .await
        .unwrap()
    else {
        panic!("search settings expected")
    };
    settings
}

async fn save(
    client: &Client,
    settings: search::Settings,
    credentials: BTreeMap<Driver, search::Credentials>,
) -> search::Settings {
    let Output::SearchSettings(settings) = client
        .execute(client.prepare(Command::SaveSearchSettings(search::Update {
            settings,
            credentials,
        })))
        .await
        .unwrap()
    else {
        panic!("search settings expected")
    };
    settings
}

#[tokio::test]
async fn preserves_private_credentials_and_revisions() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let node = Node::start_empty(&profile).await.unwrap();
    let (controller, remote) = controller(&node, directory.path()).await;
    let mut expected = search::Settings::default();
    for transport in [node.local(), remote] {
        let client = Client::new(transport);
        assert_eq!(read(&client).await, expected);
        let mut updates = client.subscribe().await.unwrap();
        let initial = updates.next().await.unwrap();
        let mut projection = Projection::new(node.id(), 0);
        projection.apply(0, initial).unwrap();
        let mut changed = expected.clone();
        changed.driver = Some(Selection::Auto);
        changed.fallback = !changed.fallback;
        changed.providers.get_mut(&Driver::Tavily).unwrap().enabled = true;
        changed
            .providers
            .get_mut(&Driver::Searxng)
            .unwrap()
            .endpoint = Some("http://192.168.10.20:8080/".into());
        let update = search::Update {
            settings: changed.clone(),
            credentials: BTreeMap::from([
                (
                    Driver::Tavily,
                    search::Credentials {
                        key: SecretUpdate::Replace(Secret::new("synthetic-search-key".into())),
                        ..Default::default()
                    },
                ),
                (
                    Driver::Searxng,
                    search::Credentials {
                        key: SecretUpdate::Replace(Secret::new("synthetic-lan-key".into())),
                        basic: SecretUpdate::Replace(Secret::new(
                            "lan-user:private-password".into(),
                        )),
                    },
                ),
            ]),
        };
        let request = client.prepare(Command::SaveSearchSettings(update));
        assert!(request.command.durable());
        let output = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), output);
        let Output::SearchSettings(saved) = output else {
            panic!("saved search settings expected")
        };
        assert_eq!(saved.revision, expected.revision + 1);
        assert!(saved.providers[&Driver::Tavily].key_configured);
        let error = client
            .execute(client.prepare(Command::SaveSearchSettings(search::Update {
                settings: changed,
                credentials: Default::default(),
            })))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let update = updates.next().await.unwrap();
                assert!(
                    !serde_json::to_string(&update)
                        .unwrap()
                        .contains("synthetic-search-key")
                );
                let serialized = serde_json::to_string(&update).unwrap();
                assert!(!serialized.contains("synthetic-lan-key"));
                assert!(!serialized.contains("private-password"));
                assert_ne!(projection.apply(0, update).unwrap(), Apply::Recover);
                if projection.snapshot().unwrap().search_settings_revision == saved.revision {
                    break;
                }
            }
        })
        .await
        .unwrap();
        let public = serde_json::to_string(projection.snapshot().unwrap()).unwrap();
        assert!(!public.contains("synthetic-search-key"));
        assert!(!public.contains("synthetic-lan-key"));
        assert!(!public.contains("private-password"));
        assert!(
            !serde_json::to_string(&read(&client).await)
                .unwrap()
                .contains("synthetic-search-key")
        );
        expected = saved;
    }
    node.shutdown().await.unwrap();
    let node = Node::start_empty(&profile).await.unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    for transport in [node.local(), controller.handle().remote(address)] {
        let client = Client::new(transport);
        assert_eq!(read(&client).await, expected);
        let mut settings = read(&client).await;
        settings.driver = None;
        expected = save(
            &client,
            settings,
            BTreeMap::from([(
                Driver::Tavily,
                search::Credentials {
                    key: SecretUpdate::Clear,
                    ..Default::default()
                },
            )]),
        )
        .await;
        assert!(!expected.providers[&Driver::Tavily].key_configured);
    }
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
}

#[tokio::test]
async fn searches_all_saved_drivers_without_task_approval() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let http = Server::start().await;
        let mut settings = read(&client).await;
        let mut credentials = BTreeMap::new();
        for driver in Driver::ALL {
            node.set_search_fixture(driver, &http.url(driver));
            settings.providers.get_mut(&driver).unwrap().enabled = true;
            if driver.key_required() || driver == Driver::Exa {
                credentials.insert(
                    driver,
                    search::Credentials {
                        key: SecretUpdate::Replace(Secret::new("synthetic-driver-key".into())),
                        ..Default::default()
                    },
                );
            }
        }
        settings
            .providers
            .get_mut(&Driver::Searxng)
            .unwrap()
            .endpoint = Some("http://192.168.10.20:8080/".into());
        credentials.insert(
            Driver::Searxng,
            search::Credentials {
                key: SecretUpdate::Replace(Secret::new("synthetic-searxng-key".into())),
                basic: SecretUpdate::Replace(Secret::new("fixture-user:fixture-password".into())),
            },
        );
        settings = save(&client, settings, credentials).await;
        for driver in Driver::ALL {
            settings.driver = Some(Selection::Driver(driver));
            settings = save(&client, settings, Default::default()).await;
            let request = client.prepare(Command::SearchWeb(search::Request {
                query: "Public source 中文 🙂".into(),
                limit: 1,
                ..Default::default()
            }));
            assert!(!request.command.durable());
            let Output::SearchPage(page) = client.execute(request).await.unwrap() else {
                panic!("search page expected")
            };
            assert_eq!(page.driver, driver);
            assert_eq!(page.results.len(), 1);
            assert_eq!(page.results[0].title, "Public source");
            assert!(page.untrusted);
            let Output::SearchPage(page) = client
                .execute(client.prepare(Command::TestSearchDriver { driver }))
                .await
                .unwrap()
            else {
                panic!("driver test page expected")
            };
            assert_eq!(page.driver, driver);
        }
        settings.driver = None;
        settings.providers.get_mut(&Driver::Tavily).unwrap().enabled = false;
        let saved = save(&client, settings, Default::default()).await;
        assert_eq!(
            client
                .execute(client.prepare(Command::SearchWeb(search::Request {
                    query: "disabled".into(),
                    limit: 5,
                    ..Default::default()
                })))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        let Output::SearchPage(page) = client
            .execute(client.prepare(Command::TestSearchDriver {
                driver: Driver::Tavily,
            }))
            .await
            .unwrap()
        else {
            panic!("disabled configured driver test expected")
        };
        assert_eq!(page.driver, Driver::Tavily);
        assert_eq!(read(&client).await, saved);
        http.respond(
            "/tavily",
            vec![Reply::Json(serde_json::json!({"bad":"response"}))],
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::TestSearchDriver {
                    driver: Driver::Tavily
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        {
            let requests = http.requests.lock().unwrap();
            let searxng = requests
                .iter()
                .find(|(header, _)| header.starts_with("GET /searxng"))
                .unwrap();
            let lower = searxng.0.to_ascii_lowercase();
            assert!(lower.contains("x-api-key: synthetic-searxng-key"));
            assert!(
                lower.contains(
                    "authorization: basic Zml4dHVyZS11c2VyOmZpeHR1cmUtcGFzc3dvcmQ="
                        .to_ascii_lowercase()
                        .as_str()
                )
            );
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn shares_fallback_cooldown_and_cancellation() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Arc::new(Client::new(if remote { transport } else { node.local() }));
        let http = Server::start().await;
        node.set_search_fixture(Driver::Exa, &format!("{}/mcp", http.endpoint));
        node.set_search_fixture(Driver::Tavily, &http.url(Driver::Tavily));
        let mut settings = read(&client).await;
        settings.providers.get_mut(&Driver::Tavily).unwrap().enabled = true;
        settings = save(
            &client,
            settings,
            BTreeMap::from([(
                Driver::Tavily,
                search::Credentials {
                    key: SecretUpdate::Replace(Secret::new("synthetic-key".into())),
                    ..Default::default()
                },
            )]),
        )
        .await;
        http.respond("/mcp", vec![Reply::Status(429, Some("30".into()))]);
        for _ in 0..2 {
            let Output::SearchPage(page) = client
                .execute(client.prepare(Command::SearchWeb(search::Request {
                    query: "source".into(),
                    limit: 5,
                    ..Default::default()
                })))
                .await
                .unwrap()
            else {
                panic!("fallback page expected")
            };
            assert_eq!(page.driver, Driver::Tavily);
        }
        assert_eq!(http.count("/mcp"), 1);
        assert_eq!(http.count("/tavily"), 2);
        settings.driver = Some(Selection::Driver(Driver::Tavily));
        settings.fallback = false;
        save(&client, settings, Default::default()).await;
        http.holds.lock().unwrap().insert("/tavily".into());
        let request = client.prepare(Command::SearchWeb(search::Request {
            query: "wait".into(),
            limit: 5,
            ..Default::default()
        }));
        let id = request.id;
        let active = client.clone();
        let pending = tokio::spawn(async move { active.execute(request).await });
        tokio::time::timeout(Duration::from_secs(5), http.held.cancelled())
            .await
            .unwrap();
        let other = RequestId::new();
        client
            .execute(client.prepare(Command::CancelWebSearch { request: other }))
            .await
            .unwrap();
        assert!(!pending.is_finished());
        assert_eq!(
            client
                .execute(client.prepare(Command::CancelWebSearch { request: id }))
                .await
                .unwrap(),
            Output::WebSearchCancelled { request: id }
        );
        assert_eq!(
            pending.await.unwrap().unwrap_err().code,
            ErrorCode::Cancelled
        );
        tokio::time::timeout(Duration::from_secs(5), http.disconnected.cancelled())
            .await
            .unwrap();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_invalid_settings_without_storing_keys() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let original = read(&client).await;
        for endpoint in [
            "https://user:password@example.com/",
            "http://user:password@search.lan/",
            "file:///private/search",
            "https://example.com/?key=private",
        ] {
            let mut settings = original.clone();
            settings
                .providers
                .get_mut(&Driver::Searxng)
                .unwrap()
                .endpoint = Some(endpoint.into());
            let error = client
                .execute(client.prepare(Command::SaveSearchSettings(search::Update {
                    settings,
                    credentials: Default::default(),
                })))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert_eq!(read(&client).await, original);
        }
        assert_eq!(
            client
                .execute(client.prepare(Command::TestSearchDriver {
                    driver: Driver::Tavily
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
