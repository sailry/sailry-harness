use super::*;
use sailry_link::Transport;
use std::sync::Arc;

#[path = "distribution/refresh.rs"]
mod refresh;

#[tokio::test]
async fn catalog_preserves_localized_metadata() {
    let index: serde_json::Value =
        serde_json::from_str(include_str!("../../../../plugins/catalog.json")).unwrap();
    let mut expected: Vec<_> = index["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| {
            (
                package["id"].as_str().unwrap(),
                package["manifest"]["name"].as_str().unwrap(),
            )
        })
        .collect();
    expected.sort_unstable();
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
        let mut actual: Vec<_> = catalog
            .entries
            .iter()
            .map(|entry| (entry.id.as_str(), entry.name.as_str()))
            .collect();
        actual.sort_unstable();
        assert_eq!(actual, expected);
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
            bundled: true,
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
                bundled: true,
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_unexpected_profile_without_changes() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    node.shutdown().await.unwrap();
    let path = profile.join("storage/node.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE node ADD COLUMN unexpected TEXT DEFAULT 'retained';")
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
async fn restores_defaults_without_resetting_plugins() {
    for remote in [false, true] {
        let (directory, node, controller, client, _) = fixture(remote).await;
        let memory = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "memory".into(),
                },
            )
            .await,
        );
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "memory".into(),
                expected_revision: memory.summary.revision,
                enabled: false,
            },
        )
        .await;
        let browser = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "browser".into(),
                },
            )
            .await,
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: "browser".into(),
                expected_revision: browser.summary.revision,
            },
        )
        .await;
        let before = execute(&client, Command::ListPlugins).await;
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let profile = directory.path().join("node");
        let path = profile.join("storage/node.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("ALTER TABLE node DROP COLUMN plugin_defaults;")
            .unwrap();
        drop(db);
        let node = Node::start(&profile).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transport: Arc<dyn Transport> = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport);
        assert_eq!(execute(&client, Command::ListPlugins).await, before);
        let db = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("SELECT plugin_defaults FROM node", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        drop(db);
        let backup = fs::read_dir(profile.join("storage/backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(backup.len(), 1);
        let previous = rusqlite::Connection::open_with_flags(
            &backup[0],
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert!(
            previous
                .prepare("SELECT plugin_defaults FROM node")
                .is_err()
        );
        assert_eq!(
            previous
                .query_row("SELECT count(*) FROM plugins", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            db_count(&path)
        );
        drop(previous);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

fn db_count(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap()
        .query_row("SELECT count(*) FROM plugins", [], |row| row.get(0))
        .unwrap()
}

async fn removed_default(name: &str, enabled: bool) {
    for remote in [false, true] {
        let (directory, node, controller, client, _) = fixture(remote).await;
        let installed = info(execute(&client, Command::ReadPlugin { name: name.into() }).await);
        assert_eq!(installed.origin, Some(plugin::Origin::Bundled));
        assert_eq!(installed.summary.enabled, enabled);
        assert_eq!(installed.summary.revision, 1);
        assert_eq!(installed.summary.digest.len(), 64);
        if name == "code" {
            use sailry_protocol::tool::Operation;
            let extension = installed.extension.as_ref().unwrap();
            let desktop = extension.desktop.as_ref().unwrap();
            let page = extension.settings_page.as_ref().unwrap();
            assert!(page.enabled_control);
            assert_eq!(
                page.entry.as_deref(),
                Some("dev.sailry.platform/desktop/settings.js")
            );
            assert!(desktop.resources.contains(page.entry.as_ref().unwrap()));
            let Output::PluginView(bundle) = execute(
                &client,
                Command::ReadPluginView {
                    package: installed.summary.reference(),
                    surface: plugin::desktop::Surface::Settings,
                },
            )
            .await
            else {
                panic!("view expected")
            };
            assert!(bundle.valid());
            assert_eq!(Some(&bundle.entry), page.entry.as_ref());
            assert!(extension.host.is_none());
            assert!(installed.skills.is_empty());
            assert!(installed.mcp.is_empty());
            assert!(extension.model_tools.is_empty());
            assert_eq!(
                extension.actions,
                [
                    plugin::Action::ReadCode,
                    plugin::Action::ManageCode,
                    plugin::Action::ReadCodeSettings,
                    plugin::Action::SaveCodeSettings,
                ]
            );
            assert_eq!(
                extension
                    .tools
                    .iter()
                    .map(|tool| tool.operation)
                    .collect::<Vec<_>>(),
                vec![
                    Some(Operation::CodeDiagnostics),
                    Some(Operation::CodeDefinition),
                    Some(Operation::CodeReferences),
                    Some(Operation::CodeHover),
                    Some(Operation::CodeSymbols),
                    Some(Operation::CodePackages),
                    Some(Operation::InstallCodeServer),
                    Some(Operation::UninstallCodeServer),
                ]
            );
            assert!(
                extension
                    .tools
                    .iter()
                    .all(|tool| tool.handler.is_none() && tool.server.is_none())
            );
        } else if let Some(desktop) = &installed.extension.as_ref().unwrap().desktop {
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
        } else if name == "web-search" {
            use sailry_protocol::tool::Operation;
            let extension = installed.extension.as_ref().unwrap();
            assert_eq!(extension.model_tools.len(), 1);
            assert_eq!(
                extension.model_tools[0].capability,
                plugin::model_tools::Capability::WebSearch
            );
            assert_eq!(extension.actions, [plugin::Action::ReadWeb]);
            assert_eq!(
                extension
                    .tools
                    .iter()
                    .map(|tool| tool.operation)
                    .collect::<Vec<_>>(),
                [Some(Operation::FetchWeb), Some(Operation::SearchWeb)]
            );
            assert!(
                extension
                    .tools
                    .iter()
                    .all(|tool| tool.handler.is_none() && tool.server.is_none())
            );
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
                .any(|entry| entry.name == name || entry.name == format!("sailry.builtin.{name}"))
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
        if name == "code" {
            let Output::CodeSettings(settings) = execute(&client, Command::ReadCodeSettings).await
            else {
                panic!("language settings expected")
            };
            assert!(settings.enabled);
        }
        assert_eq!(reinstalled.origin, Some(plugin::Origin::Bundled));
        assert_eq!(reinstalled.summary.revision, 5);
        assert_eq!(reinstalled.summary.digest, installed.summary.digest);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

mod removed_defaults {
    async fn service_only(client: &super::Client) {
        use super::*;
        let Output::Plugins(entries) = execute(client, Command::ListPlugins).await else {
            panic!("inventory expected")
        };
        assert!(
            !entries
                .iter()
                .any(|entry| entry.name == "knowledge" || entry.name == "sailry.builtin.knowledge")
        );
        let Output::PluginCatalog(catalog) = execute(
            client,
            Command::SearchPluginCatalog {
                source: plugin::catalog::Source::Official,
                query: String::new(),
                page: 1,
            },
        )
        .await
        else {
            panic!("catalog expected")
        };
        assert!(
            !catalog
                .entries
                .iter()
                .any(|entry| entry.name == "knowledge")
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::InstallBundledPlugin {
                    name: "knowledge".into(),
                    expected_revision: 0,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert!(matches!(
            execute(client, Command::ReadKnowledgeSettings).await,
            Output::KnowledgeSettings(_)
        ));
        assert!(
            matches!(execute(client, Command::ReadKnowledgeModels).await, Output::KnowledgeModels(models) if !models.is_empty())
        );
    }

    #[tokio::test]
    async fn knowledge() {
        use super::*;
        for remote in [false, true] {
            let (directory, node, controller, client, _) = fixture(remote).await;
            service_only(&client).await;
            node.shutdown().await.unwrap();
            let node = Node::start_with_plugin_catalog(
                directory.path().join("node"),
                "http://127.0.0.1:9/",
            )
            .await
            .unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            service_only(&client).await;
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
        }
    }

    macro_rules! check {
        ($($test:ident => ($name:literal, $enabled:literal)),* $(,)?) => {
            $(
                #[tokio::test]
                async fn $test() {
                    super::removed_default($name, $enabled).await;
                }
            )*
        };
    }

    check! {
        reminders => ("reminders", true),
        scheduled_tasks => ("scheduled-tasks", true),
        web_search => ("web-search", true),
        goals => ("goals", true),
        commands => ("commands", true),
        external_browser => ("external-browser", false),
        browser => ("browser", true),
        computer => ("computer", false),
        media => ("media", true),
        memory => ("memory", true),
        files => ("files", true),
        code => ("code", true),
        progress => ("progress", true),
        delegation => ("delegation", true),
        statistics => ("statistics", true),
        databases => ("databases", true),
        ssh => ("ssh", true),
        worktrees => ("worktrees", true),
        git => ("git", true),
        context7 => ("context7", true),
        github => ("github", true),
        code_review => ("code-review", true),
        office => ("office", true),
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

#[tokio::test]
async fn preserves_unshipped_installed_package() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let source = directory.path().join("source/notes-fixture");
        fs::create_dir_all(&source).unwrap();
        let manifest = serde_json::json!({
            "$schema": plugin::schema::AGENT_PLUGINS_ID,
            "name": "knowledge", "version": "0.1.0", "description": "Installed notes fixture",
            "extensions": {"dev.sailry.platform": {
                "api_version": "v1", "actions": ["knowledge.read", "storage.read"],
                "tools": [{"name": "search_notes", "operation": "knowledge.query", "description": "Search owned notes", "contexts": ["plugin", "workspace"]}]
            }}
        });
        fs::write(
            source.join("plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let installed = info(
            execute(
                &client,
                Command::InstallPlugin {
                    worktree,
                    path: "notes-fixture".into(),
                    name: "knowledge".into(),
                    expected_revision: 0,
                    expected_digest: None,
                },
            )
            .await,
        );
        let mut original = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "knowledge".into(),
                    expected_revision: installed.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        let profile = node.profile().to_owned();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        // An earlier distribution may have supplied this ordinary installed package.
        // Removing it from today's catalog must not delete private installed records.
        original.origin = Some(plugin::Origin::Bundled);
        let database = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        database
            .execute(
                "UPDATE plugins SET body=?1 WHERE name='knowledge'",
                [serde_json::to_vec(&original).unwrap()],
            )
            .unwrap();
        drop(database);
        let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let controller = Link::controller(
            directory.path().join("updated-controller"),
            NetworkScope::default(),
        )
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
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "knowledge".into()
                    }
                )
                .await
            ),
            original
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::InstallBundledPlugin {
                    name: "knowledge".into(),
                    expected_revision: original.summary.revision,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "knowledge".into()
                    }
                )
                .await
            ),
            original
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
