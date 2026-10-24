use super::*;
use sailry_link::Transport;
use std::sync::Arc;

#[path = "distribution/refresh.rs"]
mod refresh;

#[tokio::test]
async fn loads_large_bundled_manifests() {
    for remote in [false, true] {
        let (_directory, node, controller, client, _) = fixture(remote).await;
        for (name, bytes) in [
            (
                "files",
                include_bytes!("../../../../plugins/files/plugin.json").as_slice(),
            ),
            (
                "git",
                include_bytes!("../../../../plugins/git/plugin.json").as_slice(),
            ),
        ] {
            assert!(
                bytes.len() > 64 * 1024,
                "regression requires a large manifest"
            );
            let manifest: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let expected: plugin::Extension =
                serde_json::from_value(manifest["extensions"][plugin::NAMESPACE].clone()).unwrap();
            let installed = info(execute(&client, Command::ReadPlugin { name: name.into() }).await);
            assert_eq!(installed.origin, Some(plugin::Origin::Bundled));
            assert!(
                installed.issues.is_empty(),
                "{name}: {:?}",
                installed.issues
            );
            assert_eq!(installed.extension.as_ref(), Some(&expected));
            let Output::CatalogDetails(plugin::catalog::Details::Plugin(details)) = execute(
                &client,
                Command::ReadCatalogPluginInfo {
                    source: plugin::catalog::Source::Official,
                    id: name.into(),
                    bundled: true,
                },
            )
            .await
            else {
                panic!("catalog details expected");
            };
            assert_eq!(details.extension.as_ref(), Some(&expected));
            let Output::PluginView(bundle) = execute(
                &client,
                Command::ReadPluginView {
                    surface: plugin::desktop::Surface::Workspace,
                    package: installed.summary.reference(),
                },
            )
            .await
            else {
                panic!("plugin view expected");
            };
            assert!(bundle.valid());
            assert_eq!(bundle.package, installed.summary.reference());
            let desktop = expected.desktop.unwrap();
            assert_eq!(bundle.entry, desktop.entry.unwrap());
            assert_eq!(
                bundle.files.len() + bundle.images.len(),
                desktop.resources.len()
            );
            assert!(bundle.files.contains_key(&bundle.entry));
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

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
                force: false,
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
                force: false,
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
        let Output::CatalogDetails(plugin::catalog::Details::Plugin(details)) =
            admission.completion.await.unwrap().unwrap()
        else {
            panic!("catalog details expected")
        };
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
    let path = profile.join("storage/node.turso");
    let (db_handle, db) = crate::plugins::database_fixture::open(&path, false).await;
    db.execute_batch("ALTER TABLE node ADD COLUMN unexpected TEXT DEFAULT 'retained';")
        .await
        .unwrap();
    crate::plugins::database_fixture::close(db_handle, db).await;
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
        let path = profile.join("storage/node.turso");
        let (db_handle, db) = crate::plugins::database_fixture::open(&path, false).await;
        db.execute_batch(
            "ALTER TABLE node DROP COLUMN plugin_defaults;
             ALTER TABLE agent_approvals DROP COLUMN prepared;",
        )
        .await
        .unwrap();
        crate::plugins::database_fixture::close(db_handle, db).await;
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

        assert_eq!(
            {
                let parameters = ();
                let sql = "SELECT plugin_defaults FROM node";
                node.with_storage(move |db| {
                    Box::pin(async move {
                        let row = db
                            .prepare(sql)
                            .await
                            .map_err(crate::plugins::database_fixture::fault)?
                            .query_row(parameters)
                            .await
                            .map_err(crate::plugins::database_fixture::fault)?;
                        let result: turso::Result<_> = row.get::<i64>(0);
                        result.map_err(crate::plugins::database_fixture::fault)
                    })
                })
                .await
                .unwrap()
            },
            1
        );
        let (kind, required, default) = {
            let parameters = ();
            let sql = "SELECT type, \"notnull\", dflt_value FROM pragma_table_info('agent_approvals') WHERE name='prepared'";
            node.with_storage(move |db| {
                Box::pin(async move {
                    let row = db
                        .prepare(sql)
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?
                        .query_row(parameters)
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?;
                    let result: turso::Result<_> = (|row: &turso::Row| {
                        Ok((
                            row.get::<String>(0)?,
                            row.get::<i64>(1)?,
                            row.get::<String>(2)?,
                        ))
                    })(&row);
                    result.map_err(crate::plugins::database_fixture::fault)
                })
            })
            .await
            .unwrap()
        };
        assert_eq!(kind, "BLOB");
        assert_eq!(required, 1);
        let prepared: Vec<u8> = {
            let parameters = ();
            let sql = format!("SELECT {default}");
            node.with_storage(move |db| {
                Box::pin(async move {
                    let row = db
                        .prepare(sql)
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?
                        .query_row(parameters)
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?;
                    let result: turso::Result<_> = row.get(0);
                    result.map_err(crate::plugins::database_fixture::fault)
                })
            })
            .await
            .unwrap()
        };
        assert_eq!(prepared.as_slice(), b"null");
        assert_eq!(
            {
                let parameters = ();
                let sql = "SELECT count(*) FROM agent_approvals";
                node.with_storage(move |db| {
                    Box::pin(async move {
                        let row = db
                            .prepare(sql)
                            .await
                            .map_err(crate::plugins::database_fixture::fault)?
                            .query_row(parameters)
                            .await
                            .map_err(crate::plugins::database_fixture::fault)?;
                        let result: turso::Result<_> = row.get::<i64>(0);
                        result.map_err(crate::plugins::database_fixture::fault)
                    })
                })
                .await
                .unwrap()
            },
            0
        );
        let backup = fs::read_dir(profile.join("storage/backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(backup.len(), 1);
        let (previous_handle, previous) =
            crate::plugins::database_fixture::open(&(backup[0].join("node.turso")), true).await;
        assert!(
            previous
                .prepare("SELECT plugin_defaults FROM node")
                .await
                .is_err()
        );
        assert!(
            previous
                .prepare("SELECT prepared FROM agent_approvals")
                .await
                .is_err()
        );
        assert_eq!(
            (async {
                let mut statement = previous.prepare("SELECT count(*) FROM plugins").await?;
                let row = statement.query_row(()).await?;
                let mapped: turso::Result<_> = row.get::<i64>(0);
                mapped
            })
            .await
            .unwrap(),
            db_count(&node).await
        );
        crate::plugins::database_fixture::close(previous_handle, previous).await;
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

async fn db_count(node: &Node) -> i64 {
    node.with_storage(move |db| {
        Box::pin(async move {
            db.prepare("SELECT count(*) FROM plugins")
                .await
                .map_err(crate::plugins::database_fixture::fault)?
                .query_row(())
                .await
                .map_err(crate::plugins::database_fixture::fault)?
                .get(0)
                .map_err(crate::plugins::database_fixture::fault)
        })
    })
    .await
    .unwrap()
}

async fn removed_default(name: &str, enabled: bool) {
    for remote in [false, true] {
        let (directory, node, controller, client, _) = fixture(remote).await;
        if name == "office" {
            node.register_python_environment_fixture(name).unwrap();
        }
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
                    Some(Operation::CodeCatalog),
                    Some(Operation::CodeServers),
                    Some(Operation::StartCodeServer),
                    Some(Operation::StopCodeServer),
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
                    .all(|tool| match (&tool.handler, &tool.server) {
                        (Some(handler), None) => host.handlers.contains(&handler.name),
                        (None, Some(server)) =>
                            installed.mcp.iter().any(|item| item.name == *server),
                        _ => false,
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
        if name == "office" {
            node.register_python_environment_fixture(name).unwrap();
        }
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

#[tokio::test]
async fn activity_board_is_optional() {
    for remote in [false, true] {
        let (directory, node, controller, client, _) = fixture(remote).await;
        let Output::Plugins(entries) = execute(&client, Command::ListPlugins).await else {
            panic!("inventory expected")
        };
        assert!(!entries.iter().any(|entry| entry.name == "activity-board"));
        let plan = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "progress".into(),
                },
            )
            .await,
        );
        assert!(plan.summary.enabled);
        let extension = plan.extension.as_ref().unwrap();
        assert!(extension.desktop.is_none());
        assert!(extension.actions.is_empty());
        assert_eq!(extension.tools.len(), 1);
        assert_eq!(extension.tools[0].name, "update_plan");
        execute(
            &client,
            Command::RemovePlugin {
                name: "progress".into(),
                expected_revision: plan.summary.revision,
            },
        )
        .await;
        let available = catalog_info(
            execute(
                &client,
                Command::ReadCatalogPluginInfo {
                    source: plugin::catalog::Source::Official,
                    id: "activity-board".into(),
                    bundled: true,
                },
            )
            .await,
        );
        assert_eq!(available.summary.name, "activity-board");
        assert_eq!(available.summary.revision, 0);
        let installed = info(
            execute(
                &client,
                Command::InstallBundledPlugin {
                    name: "activity-board".into(),
                    expected_revision: 0,
                },
            )
            .await,
        );
        assert_eq!(installed.origin, Some(plugin::Origin::Bundled));
        assert_eq!(installed.summary.digest, available.summary.digest);
        assert!(installed.issues.is_empty());
        let extension = installed.extension.as_ref().unwrap();
        assert_eq!(extension.scope, plugin::Scope::Desktop);
        assert_eq!(extension.actions, [plugin::Action::ReadActivity]);
        assert!(extension.host.is_none());
        assert!(extension.tools.is_empty());
        assert!(extension.storage.is_none());
        let context = plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: installed.summary.reference(),
            worktree: None,
            session: None,
        };
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let Output::ActivityCatalog(catalog) = client
            .execute(
                client
                    .prepare(Command::ReadActivityCatalog)
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("activity catalog expected")
        };
        assert_eq!(catalog.node, snapshot.node);
        assert_eq!(catalog.cursor, snapshot.cursor);
        assert_eq!(catalog.projects, snapshot.projects);
        assert_eq!(catalog.worktrees, snapshot.worktrees);
        assert_eq!(catalog.sessions, snapshot.sessions);
        assert_eq!(catalog.terminals, snapshot.terminals);
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ReadActivity { sessions: vec![] })
                        .with_plugin(context),
                )
                .await
                .unwrap(),
            Output::Activity(vec![])
        );
        assert_eq!(
            execute(&client, Command::Snapshot).await,
            Output::Snapshot(snapshot)
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadPlugin {
                    name: "progress".into(),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "activity-board".into(),
                    expected_revision: installed.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        assert!(!disabled.summary.enabled);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "activity-board".into(),
                    },
                )
                .await,
            ),
            disabled
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: "activity-board".into(),
                expected_revision: disabled.summary.revision,
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
        for name in ["progress", "activity-board"] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadPlugin { name: name.into() }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
        }
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
                force: false,
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
            execute(client, Command::ReadLocalModelSettings).await,
            Output::LocalModelSettings(_)
        ));
        assert!(
            matches!(execute(client, Command::ReadLocalModels { kind: Some(sailry_protocol::models::Kind::Embedding) }).await, Output::LocalModels(sailry_protocol::models::Catalog { models, .. }) if !models.is_empty())
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
        let (database_handle, database) =
            crate::plugins::database_fixture::open(&(profile.join("storage/node.turso")), false)
                .await;
        database
            .execute(
                "UPDATE plugins SET body=?1 WHERE name='knowledge'",
                [serde_json::to_vec(&original).unwrap()],
            )
            .await
            .unwrap();
        crate::plugins::database_fixture::close(database_handle, database).await;
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

fn catalog_info(output: Output) -> plugin::Info {
    let Output::CatalogDetails(plugin::catalog::Details::Plugin(info)) = output else {
        panic!("portable catalog details expected")
    };
    *info
}
