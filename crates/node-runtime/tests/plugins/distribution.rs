use super::*;

#[tokio::test]
async fn catalog_preserves_localized_metadata() {
    for remote in [false, true] {
        let (_directory, node, controller, client, _) = fixture(remote).await;
        let Output::PluginCatalog(catalog) = execute(
            &client,
            Command::SearchPluginCatalog {
                source: plugin::catalog::Source::Official,
                query: String::new(),
                page: 1,
            },
        )
        .await
        else {
            panic!("catalog expected");
        };
        assert_eq!(catalog.entries.len(), 29);
        assert!(catalog.entries.iter().all(|entry| {
            !["project-summary", "task-notes", "tool-content"].contains(&entry.name.as_str())
        }));
        for entry in &catalog.entries {
            assert!(
                entry.description_locales.contains_key("zh-CN"),
                "{}",
                entry.name
            );
            assert!(entry.description("en").is_some(), "{}", entry.name);
            assert!(entry.description("zh-CN").is_some(), "{}", entry.name);
        }
        let summary = catalog
            .entries
            .iter()
            .find(|entry| entry.name == "city-trader")
            .unwrap();
        assert_eq!(
            summary.display.as_ref().unwrap().label("en"),
            "Dream City Tycoon"
        );
        assert_eq!(summary.display.as_ref().unwrap().label("zh-CN"), "筑梦大亨");
        let Output::PluginCatalog(filtered) = execute(
            &client,
            Command::SearchPluginCatalog {
                source: plugin::catalog::Source::Official,
                query: summary.description("zh-CN").unwrap().into(),
                page: 1,
            },
        )
        .await
        else {
            panic!("catalog expected");
        };
        assert_eq!(filtered.entries.len(), 1);
        assert_eq!(filtered.entries[0].name, summary.name);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn catalog_details_do_not_install_packages() {
    for remote in [false, true] {
        let (_directory, node, controller, client, _) = fixture(remote).await;
        let before = execute(&client, Command::ListPlugins).await;
        let request = client.prepare(Command::ReadCatalogPluginInfo {
            source: plugin::catalog::Source::Official,
            id: "city-trader".into(),
        });
        assert!(!request.command.durable());
        let admission = client.dispatch(request).await.unwrap();
        assert!(!admission.receipt.durable);
        let details = info(admission.completion.await.unwrap().unwrap());
        assert_eq!(details.summary.name, "city-trader");
        assert_eq!(details.summary.revision, 0);
        assert_eq!(details.summary.digest.len(), 64);
        assert!(
            details
                .extension
                .as_ref()
                .unwrap()
                .settings_schema
                .is_some()
        );
        assert_eq!(execute(&client, Command::ListPlugins).await, before);
        let error = client
            .execute(client.prepare(Command::ReadPlugin {
                name: "city-trader".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
        let error = client
            .execute(client.prepare(Command::ReadCatalogPluginInfo {
                source: plugin::catalog::Source::Official,
                id: "../city-trader".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn incompatible_profile_is_not_initialized_or_converted() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    node.shutdown().await.unwrap();
    let path = profile.join("storage/node.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE node DROP COLUMN plugin_defaults;")
        .unwrap();
    drop(db);
    let before = fs::read(&path).unwrap();
    assert!(Node::start(&profile).await.is_err());
    assert!(
        fs::read(&path).unwrap() == before,
        "existing profile changed"
    );
}

#[tokio::test]
async fn defaults_are_ordinary_packages_and_do_not_resurrect() {
    for remote in [false, true] {
        for (name, enabled) in [
            ("reminders", true),
            ("scheduled-tasks", true),
            ("web-search", true),
            ("goals", true),
            ("commands", true),
            ("external-browser", false),
            ("browser", true),
            ("computer", false),
            ("media", true),
            ("memory", true),
            ("files", true),
            ("progress", true),
            ("delegation", true),
            ("statistics", true),
            ("databases", true),
            ("ssh", true),
            ("worktrees", true),
            ("git", true),
            ("context7", true),
            ("github", true),
            ("code-review", true),
            ("office", true),
        ] {
            let (directory, node, controller, client, _) = fixture(remote).await;
            let installed = info(execute(&client, Command::ReadPlugin { name: name.into() }).await);
            assert_eq!(installed.origin, Some(plugin::Origin::Bundled));
            assert_eq!(installed.summary.enabled, enabled);
            assert_eq!(installed.summary.revision, 1);
            assert_eq!(installed.summary.digest.len(), 64);
            if let Some(desktop) = &installed.extension.as_ref().unwrap().desktop {
                let (surface, entry) = if let Some(entry) = &desktop.entry {
                    (plugin::desktop::Surface::Workspace, entry)
                } else if let Some(entry) = &desktop.ui_entry {
                    (plugin::desktop::Surface::Composer, entry)
                } else {
                    (
                        plugin::desktop::Surface::Settings,
                        installed
                            .extension
                            .as_ref()
                            .unwrap()
                            .settings_page
                            .as_ref()
                            .unwrap()
                            .entry
                            .as_ref()
                            .unwrap(),
                    )
                };
                let Output::PluginView(bundle) = execute(
                    &client,
                    Command::ReadPluginView {
                        package: installed.summary.reference(),
                        surface,
                    },
                )
                .await
                else {
                    panic!("view expected")
                };
                assert!(bundle.valid());
                assert_eq!(&bundle.entry, entry);
            } else if let Some(host) = &installed.extension.as_ref().unwrap().host {
                assert!(host.valid());
                assert!(
                    installed
                        .extension
                        .as_ref()
                        .unwrap()
                        .tools
                        .iter()
                        .all(|tool| {
                            tool.handler
                                .as_ref()
                                .is_some_and(|handler| host.handlers.contains(&handler.name))
                        })
                );
            } else if installed.skills.is_empty() && installed.mcp.is_empty() {
                assert_eq!(installed.extension.as_ref().unwrap().model_tools.len(), 1);
            } else {
                assert!(!installed.skills.is_empty());
                let extension = installed.extension.as_ref().unwrap();
                assert!(extension.model_tools.is_empty());
                assert!(extension.tools.iter().all(|tool| {
                    tool.handler.is_none()
                        && tool.server.as_ref().is_some_and(|server| {
                            installed.mcp.iter().any(|entry| &entry.name == server)
                        })
                }));
            }
            let disabled = info(
                execute(
                    &client,
                    Command::SetPluginEnabled {
                        name: name.into(),
                        expected_revision: 1,
                        enabled: false,
                    },
                )
                .await,
            );
            assert!(!disabled.summary.enabled);
            let update = client.prepare(Command::InstallBundledPlugin {
                name: name.into(),
                expected_revision: 2,
            });
            let result = client.execute(update.clone()).await.unwrap();
            let updated = info(result.clone());
            assert_eq!(updated.origin, Some(plugin::Origin::Bundled));
            assert_eq!(updated.summary.revision, 3);
            assert!(!updated.summary.enabled);
            execute(
                &client,
                Command::RemovePlugin {
                    name: name.into(),
                    expected_revision: 3,
                },
            )
            .await;
            drop(client);
            node.shutdown().await.unwrap();
            let node = Node::start(directory.path().join("node")).await.unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            assert_eq!(client.execute(update).await.unwrap(), result);
            let Output::Plugins(entries) = execute(&client, Command::ListPlugins).await else {
                panic!("inventory expected")
            };
            assert!(
                !entries
                    .iter()
                    .any(|entry| entry.name == name
                        || entry.name == format!("sailry.builtin.{name}"))
            );
            let reinstalled = info(
                execute(
                    &client,
                    Command::InstallBundledPlugin {
                        name: name.into(),
                        expected_revision: 0,
                    },
                )
                .await,
            );
            assert!(reinstalled.summary.enabled);
            assert_eq!(reinstalled.origin, Some(plugin::Origin::Bundled));
            assert_eq!(reinstalled.summary.revision, 5);
            assert_eq!(reinstalled.summary.digest, installed.summary.digest);
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn update_preserves_configuration_and_disabled_state() {
    for remote in [false, true] {
        let (directory, node, controller, client, _) = fixture(remote).await;
        let name = "gomoku";
        let installed = info(
            execute(
                &client,
                Command::InstallBundledPlugin {
                    name: name.into(),
                    expected_revision: 0,
                },
            )
            .await,
        );
        let Output::PluginSettings(configured) = execute(
            &client,
            Command::SavePluginSettings {
                package: installed.summary.reference(),
                values: std::collections::BTreeMap::from([(
                    "ai_enabled".into(),
                    serde_json::json!(false),
                )]),
                secrets: Default::default(),
            },
        )
        .await
        else {
            panic!("plugin settings expected")
        };
        assert_eq!(configured.package.settings_revision, 1);
        assert_eq!(configured.values["ai_enabled"], false);
        let configured_info =
            info(execute(&client, Command::ReadPlugin { name: name.into() }).await);
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: name.into(),
                    expected_revision: configured_info.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::InstallBundledPlugin {
                    name: name.into(),
                    expected_revision: installed.summary.revision,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let request = client.prepare(Command::InstallBundledPlugin {
            name: name.into(),
            expected_revision: disabled.summary.revision,
        });
        let updated = info(client.execute(request.clone()).await.unwrap());
        assert_eq!(updated.origin, Some(plugin::Origin::Bundled));
        assert!(!updated.summary.enabled);
        assert_eq!(updated.summary.revision, disabled.summary.revision + 1);
        assert_eq!(updated.summary.reference(), configured.package);
        assert_eq!(updated.summary.digest, installed.summary.digest);
        let read = || Command::ReadPluginSettings {
            package: updated.summary.reference(),
        };
        assert_eq!(
            execute(&client, read()).await,
            Output::PluginSettings(configured.clone())
        );
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(info(client.execute(request).await.unwrap()), updated);
        assert_eq!(
            info(execute(&client, Command::ReadPlugin { name: name.into() }).await),
            updated
        );
        assert_eq!(
            execute(&client, read()).await,
            Output::PluginSettings(configured)
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
