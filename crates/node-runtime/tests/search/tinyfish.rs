use super::*;

#[tokio::test]
async fn selects_and_tests_configured_service() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let http = Server::start().await;
        node.set_search_fixture(Driver::Exa, &http.url(Driver::Exa));
        node.set_search_fixture(Driver::Tinyfish, &http.url(Driver::Tinyfish));
        http.respond("/exa", vec![Reply::Status(503, None)]);
        let mut settings = read(&client).await;
        settings
            .providers
            .get_mut(&Driver::Tinyfish)
            .unwrap()
            .enabled = true;
        save(
            &client,
            settings,
            [Driver::Exa, Driver::Tinyfish]
                .into_iter()
                .map(|driver| {
                    (
                        driver,
                        search::Credentials {
                            key: SecretUpdate::Replace(Secret::new("private-fixture-key".into())),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        )
        .await;
        for command in [
            Command::SearchWeb(search::Request {
                query: "source".into(),
                ..Default::default()
            }),
            Command::TestSearchDriver {
                driver: Driver::Tinyfish,
            },
        ] {
            let Output::SearchPage(page) = client.execute(client.prepare(command)).await.unwrap()
            else {
                panic!("search page expected")
            };
            assert_eq!(page.driver, Driver::Tinyfish);
            assert_eq!(page.results.len(), 1);
            assert!(page.untrusted);
        }
        assert_eq!(http.count("/exa"), 1);
        assert_eq!(http.count("/tinyfish"), 2);
        assert!(
            http.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(header, _)| header.starts_with("GET /tinyfish"))
                .all(|(header, _)| header
                    .to_lowercase()
                    .contains("x-api-key: private-fixture-key"))
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn fills_defaults_when_reopening_a_profile() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let node = Node::start_empty(&profile).await.unwrap();
    let client = Client::new(node.local());
    let mut settings = read(&client).await;
    settings.fallback = false;
    settings.providers.get_mut(&Driver::Tavily).unwrap().enabled = true;
    let expected = save(
        &client,
        settings,
        BTreeMap::from([(
            Driver::Tavily,
            search::Credentials {
                key: SecretUpdate::Replace(Secret::new("preserved-fixture-key".into())),
                ..Default::default()
            },
        )]),
    )
    .await;
    node.shutdown().await.unwrap();
    drop(client);
    let database = turso::Builder::new_local(profile.join("storage/node.turso").to_str().unwrap())
        .build()
        .await
        .unwrap();
    let db = database.connect().unwrap();
    db.execute("UPDATE search_settings SET body=CAST(json_remove(body,'$.settings.providers.tinyfish') AS BLOB)",()).await.unwrap();
    drop(db);
    drop(database);
    let node = Node::start_empty(&profile).await.unwrap();
    let (controller, remote) = controller(&node, directory.path()).await;
    for transport in [node.local(), remote] {
        let client = Client::new(transport);
        assert_eq!(read(&client).await, expected);
        let http = Server::start().await;
        node.set_search_fixture(Driver::Tavily, &http.url(Driver::Tavily));
        let Output::SearchPage(page) = client
            .execute(client.prepare(Command::TestSearchDriver {
                driver: Driver::Tavily,
            }))
            .await
            .unwrap()
        else {
            panic!("search page expected")
        };
        assert_eq!(page.driver, Driver::Tavily);
        assert!(
            http.requests.lock().unwrap()[0]
                .0
                .to_lowercase()
                .contains("authorization: bearer preserved-fixture-key")
        );
    }
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
}
