use super::*;
use std::sync::atomic::Ordering;
mod cache;
mod external;
mod fixture;
mod live;
mod skills;
mod updates;
mod versions;
use fixture::{Catalog, FIRST, SECOND};

async fn connection(remote: bool, catalog: &Catalog) -> (tempfile::TempDir, Node, Link, Client) {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start_with_skill_source(directory.path().join("node"), &catalog.endpoint)
        .await
        .unwrap();
    let (controller, client) = attach(remote, &directory, &node).await;
    (directory, node, controller, client)
}

async fn attach(remote: bool, directory: &tempfile::TempDir, node: &Node) -> (Link, Client) {
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let client = Client::new(if remote {
        controller.handle().remote(address)
    } else {
        node.local()
    });
    (controller, client)
}

fn search(query: &str) -> Command {
    Command::SearchPluginCatalog {
        force: false,
        source: plugin::catalog::Source::Official,
        query: query.into(),
        page: 1,
    }
}

#[tokio::test]
async fn installs_pinned_packages() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let before = execute(&client, Command::ListPlugins).await;
        let Output::PluginCatalog(page) = execute(&client, search("New official")).await else {
            panic!("catalog expected");
        };
        assert_eq!(page.entries.len(), 1);
        let entry = &page.entries[0];
        assert_eq!(entry.id, "official-new");
        assert_eq!(entry.version.as_deref(), Some("1.0.0"));
        assert!(!entry.bundled);
        assert_eq!(
            entry.repository.as_deref(),
            Some("https://github.com/sailry/sailry-plugins")
        );
        let source = match execute(
            &client,
            Command::ReadCatalogPlugin {
                source: plugin::catalog::Source::Official,
                id: entry.id.clone(),
            },
        )
        .await
        {
            Output::PluginRepository(source) => source,
            _ => panic!("source expected"),
        };
        assert_eq!(source.path.as_deref(), Some("official-new"));
        assert_eq!(source.git_ref.as_deref(), Some(FIRST));
        let details = details(
            execute(
                &client,
                Command::ReadCatalogPluginInfo {
                    source: plugin::catalog::Source::Official,
                    id: entry.id.clone(),
                    bundled: false,
                },
            )
            .await,
        );
        assert_eq!(details.summary.name, entry.name);
        assert_eq!(execute(&client, Command::ListPlugins).await, before);
        let Output::PluginSource(selected) =
            execute(&client, Command::InspectPluginSource { source }).await
        else {
            panic!("selection expected");
        };
        assert_eq!(selected.source.commit, FIRST);
        catalog.updated.store(true, Ordering::SeqCst);
        let request = client.prepare(Command::InstallPluginSource {
            source: selected.source.clone(),
            path: selected.path.clone(),
            name: entry.name.clone(),
            expected_revision: 0,
        });
        let installed = info(client.execute(request.clone()).await.unwrap());
        assert_eq!(installed.summary.version.as_deref(), Some("1.0.0"));
        assert_eq!(installed.summary.digest, selected.info.summary.digest);
        assert_eq!(
            installed.origin,
            Some(plugin::Origin::Online {
                source: selected.source,
                path: selected.path
            })
        );
        let requests = catalog.requests.lock().unwrap().len();
        assert_eq!(info(client.execute(request).await.unwrap()), installed);
        assert_eq!(catalog.requests.lock().unwrap().len(), requests);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn updates_preinstalled() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let installed = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "reminders".into(),
                },
            )
            .await,
        );
        assert_eq!(installed.origin, Some(plugin::Origin::Bundled));
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "reminders".into(),
                    expected_revision: installed.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        catalog.updated.store(true, Ordering::SeqCst);
        let Output::PluginUpdate(report) = execute(
            &client,
            Command::CheckPluginUpdate {
                force: true,
                name: "reminders".into(),
                expected_revision: disabled.summary.revision,
            },
        )
        .await
        else {
            panic!("update expected");
        };
        assert_eq!(report.current, disabled.summary);
        let plugin::updates::Candidate::Official(candidate) = report.available.unwrap() else {
            panic!("official catalog entry expected");
        };
        assert_eq!(candidate.version.as_deref(), Some("0.2.0"));
        assert_eq!(*catalog.requests.lock().unwrap(), catalog.index_paths());
        let Output::PluginSource(selected) = execute(
            &client,
            Command::InspectPluginSource {
                source: plugin::skills::Source {
                    repository: candidate.repository.unwrap(),
                    git_ref: Some("main".into()),
                    path: Some(candidate.id),
                },
            },
        )
        .await
        else {
            panic!("immutable source selection expected");
        };
        let source = selected.source;
        let path = selected.path;
        assert_eq!(source.commit, SECOND);
        assert_eq!(path, "reminders");
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
            disabled
        );
        let updated = info(
            execute(
                &client,
                Command::InstallPluginSource {
                    source,
                    path,
                    name: "reminders".into(),
                    expected_revision: disabled.summary.revision,
                },
            )
            .await,
        );
        assert_eq!(updated.summary.digest, selected.info.summary.digest);
        assert_eq!(updated.summary.version.as_deref(), Some("0.2.0"));
        assert!(!updated.summary.enabled);
        assert_eq!(updated.summary.revision, disabled.summary.revision + 1);
        catalog.requests.lock().unwrap().clear();
        let Output::PluginUpdate(report) = execute(
            &client,
            Command::CheckPluginUpdate {
                force: true,
                name: updated.summary.name.clone(),
                expected_revision: updated.summary.revision,
            },
        )
        .await
        else {
            panic!("update report expected");
        };
        assert!(report.available.is_none());
        assert_eq!(*catalog.requests.lock().unwrap(), catalog.index_paths());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn offline_and_invalid_indexes() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        catalog.status.store(503, Ordering::SeqCst);
        let Output::PluginCatalog(page) = execute(&client, search("Dream City")).await else {
            panic!("catalog expected");
        };
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].name, "city-trader");
        assert!(page.entries[0].bundled);
        let details = details(
            execute(
                &client,
                Command::ReadCatalogPluginInfo {
                    source: plugin::catalog::Source::Official,
                    id: "city-trader".into(),
                    bundled: true,
                },
            )
            .await,
        );
        assert_eq!(details.summary.name, "city-trader");
        catalog.status.store(200, Ordering::SeqCst);
        catalog.invalid.store(true, Ordering::SeqCst);
        let Output::PluginCatalog(page) = execute(&client, search("Dream City")).await else {
            panic!("catalog expected");
        };
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].name, "city-trader");
        assert!(page.entries[0].bundled);
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadCatalogPlugin {
                    source: plugin::catalog::Source::Official,
                    id: "city-trader".into(),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        catalog.invalid.store(false, Ordering::SeqCst);
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadCatalogPlugin {
                    source: plugin::catalog::Source::Official,
                    id: "not-listed".into(),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadCatalogPluginInfo {
                    source: plugin::catalog::Source::ThirdParty,
                    id: "city-trader".into(),
                    bundled: true,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn obsolete_manifests_use_bundled_listings() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        catalog.obsolete.store(true, Ordering::SeqCst);
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let before = execute(&client, Command::ListPlugins).await;
        let Output::PluginCatalog(page) = execute(&client, search("")).await else {
            panic!("catalog expected");
        };
        assert!(!page.unavailable);
        assert!(page.entries.iter().all(|entry| entry.bundled));
        assert!(!page.entries.iter().any(|entry| entry.id == "official-new"));
        let entry = page
            .entries
            .iter()
            .find(|entry| entry.id == "files")
            .unwrap();
        let details = details(
            execute(
                &client,
                Command::ReadCatalogPluginInfo {
                    source: plugin::catalog::Source::Official,
                    id: "files".into(),
                    bundled: true,
                },
            )
            .await,
        );
        assert_eq!(details.summary.version, entry.version);
        assert!(details.issues.is_empty());
        for command in [
            Command::ReadCatalogPlugin {
                source: plugin::catalog::Source::Official,
                id: "files".into(),
            },
            Command::ReadCatalogPluginInfo {
                source: plugin::catalog::Source::Official,
                id: "files".into(),
                bundled: false,
            },
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        assert_eq!(execute(&client, Command::ListPlugins).await, before);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

fn details(output: Output) -> plugin::Info {
    let Output::CatalogDetails(plugin::catalog::Details::Plugin(info)) = output else {
        panic!("portable catalog details expected")
    };
    *info
}

#[tokio::test]
async fn prefers_complete_newer_bundled_entries() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        catalog.legacy.store(true, Ordering::SeqCst);
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let Output::PluginCatalog(page) = execute(&client, search("files")).await else {
            panic!("catalog expected")
        };
        let entry = page
            .entries
            .iter()
            .find(|entry| entry.id == "files")
            .unwrap();
        assert!(entry.bundled);
        assert!(entry.category.is_some());
        assert_eq!(entry.repository, None);
        let details = details(
            execute(
                &client,
                Command::ReadCatalogPluginInfo {
                    source: plugin::catalog::Source::Official,
                    id: entry.id.clone(),
                    bundled: entry.bundled,
                },
            )
            .await,
        );
        assert_eq!(details.summary.version, entry.version);
        assert_eq!(details.extension.as_ref().unwrap().category, entry.category);
        let Output::PluginCatalog(page) = execute(&client, search("official-new")).await else {
            panic!("catalog expected")
        };
        assert_eq!(page.entries.len(), 1);
        assert!(!page.entries[0].bundled);
        assert_eq!(page.entries[0].version.as_deref(), Some("1.0.0"));
        assert_eq!(
            page.entries[0].repository.as_deref(),
            Some("https://github.com/sailry/sailry-plugins")
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
