use super::*;

fn forced_search(query: &str) -> Command {
    Command::SearchPluginCatalog {
        source: plugin::catalog::Source::Official,
        query: query.into(),
        page: 1,
        force: true,
    }
}

fn index_requests(catalog: &Catalog) -> usize {
    catalog
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|path| path.as_str() == "/catalog.json")
        .count()
}

async fn version(client: &Client, command: Command) -> String {
    let Output::PluginCatalog(page) = execute(client, command).await else {
        panic!("catalog expected");
    };
    assert_eq!(page.entries.len(), 1);
    assert!(!page.entries[0].bundled);
    page.entries[0].version.clone().unwrap()
}

#[tokio::test]
async fn shares_queries_sources_and_update_checks() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        assert_eq!(version(&client, search("New official")).await, "1.0.0");
        assert_eq!(index_requests(&catalog), 1);
        let source = execute(
            &client,
            Command::ReadCatalogPlugin {
                source: plugin::catalog::Source::Official,
                id: "official-new".into(),
            },
        )
        .await;
        assert!(matches!(source, Output::PluginRepository(_)));
        assert_eq!(version(&client, search("official-new")).await, "1.0.0");
        let current = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "reminders".into(),
                },
            )
            .await,
        );
        let selection = plugin::updates::Request {
            name: current.summary.name.clone(),
            expected_revision: current.summary.revision,
        };
        let Output::PluginUpdates(batch) = execute(
            &client,
            Command::CheckPluginUpdates {
                packages: vec![selection.clone()],
                force: false,
            },
        )
        .await
        else {
            panic!("update reports expected");
        };
        assert_eq!(batch.reports.len(), 1);
        assert!(batch.failures.is_empty());
        assert_eq!(index_requests(&catalog), 1);
        execute(
            &client,
            Command::CheckPluginUpdates {
                packages: vec![selection],
                force: true,
            },
        )
        .await;
        assert_eq!(index_requests(&catalog), 2);
        execute(
            &client,
            Command::CheckPluginUpdate {
                name: current.summary.name.clone(),
                expected_revision: current.summary.revision,
                force: true,
            },
        )
        .await;
        assert_eq!(index_requests(&catalog), 3);
        assert_eq!(version(&client, search("New official")).await, "1.0.0");
        assert_eq!(index_requests(&catalog), 3);
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "reminders".into()
                    }
                )
                .await
            ),
            current
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn failed_refresh_keeps_validated_entries() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        assert_eq!(version(&client, search("New official")).await, "1.0.0");
        catalog.updated.store(true, Ordering::SeqCst);
        for (invalid, status, code) in [
            (true, 200, ErrorCode::InvalidRequest),
            (false, 503, ErrorCode::Unavailable),
        ] {
            catalog.invalid.store(invalid, Ordering::SeqCst);
            catalog.status.store(status, Ordering::SeqCst);
            let error = client
                .execute(client.prepare(forced_search("New official")))
                .await
                .unwrap_err();
            assert_eq!(error.code, code);
            assert_eq!(version(&client, search("New official")).await, "1.0.0");
        }
        assert_eq!(index_requests(&catalog), 3);
        catalog.status.store(200, Ordering::SeqCst);
        assert_eq!(
            version(&client, forced_search("New official")).await,
            "2.0.0"
        );
        assert_eq!(version(&client, search("New official")).await, "2.0.0");
        assert_eq!(index_requests(&catalog), 4);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn nodes_do_not_share_cached_entries() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (_first_directory, first_node, first_controller, first_client) =
            connection(remote, &catalog).await;
        let (_second_directory, second_node, second_controller, second_client) =
            connection(remote, &catalog).await;
        assert_eq!(
            version(&first_client, search("New official")).await,
            "1.0.0"
        );
        catalog.updated.store(true, Ordering::SeqCst);
        assert_eq!(
            version(&second_client, search("New official")).await,
            "2.0.0"
        );
        assert_eq!(
            version(&first_client, search("New official")).await,
            "1.0.0"
        );
        assert_eq!(index_requests(&catalog), 2);
        first_node.shutdown().await.unwrap();
        first_controller.close().await.unwrap();
        second_node.shutdown().await.unwrap();
        second_controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn restart_discards_cached_entries() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (directory, node, controller, client) = connection(remote, &catalog).await;
        assert_eq!(version(&client, search("New official")).await, "1.0.0");
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        catalog.updated.store(true, Ordering::SeqCst);
        let node = Node::start_with_skill_source(directory.path().join("node"), &catalog.endpoint)
            .await
            .unwrap();
        let (controller, client) = attach(remote, &directory, &node).await;
        assert_eq!(version(&client, search("New official")).await, "2.0.0");
        assert_eq!(index_requests(&catalog), 2);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
