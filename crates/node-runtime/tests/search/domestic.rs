use super::*;

#[tokio::test]
async fn tests_both_documented_services() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let http = Server::start().await;
        let mut settings = read(&client).await;
        let drivers = [Driver::Volcengine, Driver::Deepseek];
        for driver in drivers {
            settings.providers.get_mut(&driver).unwrap().enabled = true;
            node.set_search_fixture(driver, &http.url(driver));
        }
        save(
            &client,
            settings,
            drivers
                .into_iter()
                .map(|driver| {
                    (
                        driver,
                        search::Credentials {
                            key: SecretUpdate::Replace(Secret::new("private-domestic-key".into())),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        )
        .await;
        for driver in drivers {
            let Output::SearchPage(page) = client
                .execute(client.prepare(Command::TestSearchDriver { driver }))
                .await
                .unwrap()
            else {
                panic!("search page expected")
            };
            assert_eq!(page.driver, driver);
            assert_eq!(page.results[0].url, "https://example.com/source");
            assert!(page.untrusted);
            assert!(
                !serde_json::to_string(&page)
                    .unwrap()
                    .contains("private-domestic-key")
            );
        }
        assert_eq!(http.count("/volcengine"), 1);
        assert_eq!(http.count("/deepseek"), 1);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn falls_back_from_bocha_to_volcengine() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let http = Server::start().await;
        let mut settings = read(&client).await;
        for driver in [Driver::Bocha, Driver::Volcengine] {
            settings.providers.get_mut(&driver).unwrap().enabled = true;
            node.set_search_fixture(driver, &http.url(driver));
        }
        http.respond("/bocha", vec![Reply::Status(503, None)]);
        save(
            &client,
            settings,
            [Driver::Bocha, Driver::Volcengine]
                .into_iter()
                .map(|driver| {
                    (
                        driver,
                        search::Credentials {
                            key: SecretUpdate::Replace(Secret::new("private-domestic-key".into())),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        )
        .await;
        let Output::SearchPage(page) = client
            .execute(client.prepare(Command::SearchWeb(search::Request {
                query: "今天的软件新闻".into(),
                ..Default::default()
            })))
            .await
            .unwrap()
        else {
            panic!("search page expected")
        };
        assert_eq!(page.driver, Driver::Volcengine);
        {
            let requests = http.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(requests[0].0.starts_with("POST /bocha"));
            assert!(requests[1].0.starts_with("POST /volcengine"));
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
