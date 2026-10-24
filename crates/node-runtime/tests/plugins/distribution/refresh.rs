use super::*;
use sailry_protocol::{code, search};
use serde_json::json;
use std::collections::BTreeMap;
use turso::{Connection, params};

#[path = "refresh/computer.rs"]
mod computer;

async fn version(client: &Client, name: &str) -> String {
    info(execute(client, Command::ReadPlugin { name: name.into() }).await)
        .summary
        .version
        .expect("bundled semantic version expected")
}

async fn previous(
    directory: &Path,
    client: &Client,
    worktree: WorktreeId,
    name: &str,
    version: &str,
) -> Info {
    let current = info(execute(client, Command::ReadPlugin { name: name.into() }).await);
    let bytes = match name {
        "code" => include_bytes!("../../../../../plugins/code/plugin.json").as_slice(),
        "web-search" => include_bytes!("../../../../../plugins/web-search/plugin.json").as_slice(),
        "browser" => include_bytes!("../../../../../plugins/browser/plugin.json").as_slice(),
        "computer" => include_bytes!("../../../../../plugins/computer/plugin.json").as_slice(),
        _ => unreachable!(),
    };
    let mut manifest: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    manifest["version"] = json!(version);
    let extension = manifest["extensions"]["dev.sailry.platform"]
        .as_object_mut()
        .unwrap();
    extension.remove("settings_page");
    extension.remove("desktop");
    extension.remove("enabled_service");
    if name == "computer" {
        extension.remove("tools");
    }
    let path = format!("previous-{name}");
    let source = directory.join("source").join(&path);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("plugin.json"), manifest.to_string()).unwrap();
    if name == "computer" {
        fs::create_dir_all(source.join("skills/retained")).unwrap();
        fs::write(
            source.join("skills/retained/SKILL.md"),
            "---\nname: retained\ndescription: Retained fixture resource\n---\nCaptured package resource\n",
        )
        .unwrap();
    }
    let installed = info(
        execute(
            client,
            Command::InstallPlugin {
                worktree,
                path,
                name: name.into(),
                expected_revision: current.summary.revision,
                expected_digest: None,
            },
        )
        .await,
    );
    assert_eq!(installed.summary.version.as_deref(), Some(version));
    assert!(
        installed
            .extension
            .as_ref()
            .unwrap()
            .settings_page
            .is_none()
    );
    installed
}

async fn bundled(path: &Path, mut installed: Info) -> Info {
    installed.origin = Some(plugin::Origin::Bundled);
    installed.summary.settings_revision = 1;
    let (database, mut db) = crate::plugins::database_fixture::open(path, false).await;
    let transaction = db.transaction().await.unwrap();
    let name = &installed.summary.name;
    transaction
        .execute(
            "UPDATE plugins SET body=?1 WHERE name=?2",
            params![serde_json::to_vec(&installed).unwrap(), name.as_str()],
        )
        .await
        .unwrap();
    transaction
        .execute(
            "INSERT INTO plugin_settings(plugin,revision,body) VALUES(?1,1,?2)",
            params![
                name.as_str(),
                serde_json::to_vec(&json!({
                    "schema":null,"values":{"draft":"retained","model":"fixture/model"},
                    "slots":{},"authorizations":{},"mcp":null,"github":null
                }))
                .unwrap()
            ],
        )
        .await
        .unwrap();
    transaction
        .execute(
            "INSERT INTO plugin_values(name,key,revision,value) VALUES(?1,'draft',1,?2)",
            params![name.as_str(), b"{\"text\":\"retained\"}".as_slice()],
        )
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    crate::plugins::database_fixture::close(database, db).await;
    installed
}

async fn records(db: &Connection) -> turso::Result<Vec<(String, String, Vec<u8>)>> {
    let mut rows = db.query("SELECT plugin,'settings',body FROM plugin_settings WHERE plugin IN ('code','web-search','browser') UNION ALL SELECT name,key,value FROM plugin_values WHERE name IN ('code','web-search','browser') ORDER BY 1,2", ()).await?;
    let mut records = Vec::new();
    while let Some(row) = rows.next().await? {
        records.push((row.get(0)?, row.get(1)?, row.get(2)?));
    }
    Ok(records)
}

async fn disable(client: &Client, installed: Info) -> Info {
    info(
        execute(
            client,
            Command::SetPluginEnabled {
                name: installed.summary.name,
                expected_revision: installed.summary.revision,
                enabled: false,
            },
        )
        .await,
    )
}

#[tokio::test]
async fn adds_missing_language_settings_on_each_path() {
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
        let memory = disable(&client, memory).await;
        let search = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "web-search".into(),
                },
            )
            .await,
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: "web-search".into(),
                expected_revision: search.summary.revision,
            },
        )
        .await;
        let Output::CodeSettings(mut settings) = execute(&client, Command::ReadCodeSettings).await
        else {
            panic!("language settings expected")
        };
        settings.enabled = false;
        settings.after_edit = false;
        let Output::CodeSettings(settings) = execute(
            &client,
            Command::SaveCodeSettings {
                expected_revision: settings.revision,
                settings,
            },
        )
        .await
        else {
            panic!("language settings expected")
        };
        let Output::Plugins(mut before) = execute(&client, Command::ListPlugins).await else {
            panic!("inventory expected")
        };
        before.retain(|summary| summary.name != "code");
        let profile = directory.path().join("node");
        let path = profile.join("storage/node.turso");
        node.shutdown().await.unwrap();
        let (db_handle, db) = crate::plugins::database_fixture::open(&path, false).await;
        assert!(
            (async {
                let mut statement = db.prepare("SELECT plugin_defaults FROM node").await?;
                let row = statement.query_row(()).await?;
                let mapped: turso::Result<_> = row.get::<bool>(0);
                mapped
            })
            .await
            .unwrap()
        );
        db.execute("DELETE FROM plugins WHERE name='code'", ())
            .await
            .unwrap();
        let removed: Vec<u8> = (async {
            let mut statement = db
                .prepare("SELECT body FROM plugins WHERE name='web-search' AND installed=0")
                .await?;
            let row = statement.query_row(()).await?;
            let mapped: turso::Result<_> = row.get(0);
            mapped
        })
        .await
        .unwrap();
        crate::plugins::database_fixture::close(db_handle, db).await;
        let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let code = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "code".into(),
                },
            )
            .await,
        );
        assert!(!code.summary.enabled);
        assert_eq!(
            execute(&client, Command::ReadCodeSettings).await,
            Output::CodeSettings(settings)
        );
        assert_eq!(code.origin, Some(plugin::Origin::Bundled));
        let page = code.extension.unwrap().settings_page.unwrap();
        assert_eq!(
            page.entry.as_deref(),
            Some("dev.sailry.platform/desktop/settings.js")
        );
        assert_eq!(page.navigation.label("en"), "Language services");
        assert_eq!(page.placement.group, plugin::desktop::SettingsGroup::Tools);
        assert!(page.entry.is_some());
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "memory".into()
                    }
                )
                .await
            ),
            memory
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadPlugin {
                    name: "web-search".into()
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let Output::Plugins(mut after) = execute(&client, Command::ListPlugins).await else {
            panic!("inventory expected")
        };
        after.retain(|summary| summary.name != "code");
        assert_eq!(after, before);

        let retained: Vec<u8> = {
            let parameters = ();
            let sql = "SELECT body FROM plugins WHERE name='web-search' AND installed=0";
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
        assert_eq!(retained, removed);

        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

async fn capture(client: &Client, worktree: WorktreeId) -> (Session, TurnId) {
    let model = completion::configure(client, "http://127.0.0.1:9/".into()).await;
    let Output::Snapshot(snapshot) = execute(client, Command::Snapshot).await else {
        panic!("snapshot expected");
    };
    let project = snapshot
        .worktrees
        .iter()
        .find(|tree| tree.id == worktree)
        .unwrap()
        .project
        .expect("fixture worktree belongs to a project");
    let Output::Session(session) = execute(
        client,
        Command::CreateSession {
            project: Some(project),
            worktree: Some(worktree),
            config: Some(SessionConfig {
                features: Default::default(),
                provider: model.split('/').next().unwrap().parse().unwrap(),
                model: "fixture".into(),
                mode: WorkMode::Code,
                options: Default::default(),
                permission: Permission::Ask,
                effort: Effort::Default,
                credential: None,
                assistant: None,
                resource: None,
            }),
        },
    )
    .await
    else {
        panic!("session expected");
    };
    assert_eq!(session.project, Some(project));
    assert_eq!(session.worktree, worktree);
    let Output::Conversation(history) = execute(
        client,
        Command::ReadConversation {
            session: session.id,
            before: None,
            limit: 20,
        },
    )
    .await
    else {
        panic!("conversation expected");
    };
    execute(
        client,
        Command::SetQueuePaused {
            session: session.id,
            expected_revision: history.page.queue.revision,
            paused: true,
        },
    )
    .await;
    let Output::QueuedTurn(turn) = execute(
        client,
        Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Keep this admitted draft".into(),
        },
    )
    .await
    else {
        panic!("queued turn expected");
    };
    (session, turn.id)
}

async fn settings(client: &Client) -> (code::Settings, search::Settings) {
    let Output::CodeSettings(mut code) = execute(client, Command::ReadCodeSettings).await else {
        panic!("code settings expected");
    };
    code.after_edit = false;
    code.servers.insert(
        "rust-analyzer".into(),
        code::Override {
            disabled: false,
            command: Some("fixture-language-server".into()),
            args: Some(vec!["--fixture".into()]),
            config: Some(json!({"fixture":true})),
            environment: Some(BTreeMap::from([("FIXTURE".into(), "retained".into())])),
        },
    );
    let Output::CodeSettings(code) = execute(
        client,
        Command::SaveCodeSettings {
            expected_revision: code.revision,
            settings: code,
        },
    )
    .await
    else {
        panic!("code settings expected");
    };
    let Output::SearchSettings(mut search) = execute(client, Command::ReadSearchSettings).await
    else {
        panic!("search settings expected");
    };
    search.prefer_native = false;
    search.fallback = false;
    search.driver = Some(search::Selection::Driver(search::Driver::Brave));
    search
        .providers
        .get_mut(&search::Driver::Brave)
        .unwrap()
        .enabled = true;
    let Output::SearchSettings(search) = execute(
        client,
        Command::SaveSearchSettings(search::Update {
            settings: search,
            credentials: BTreeMap::from([(
                search::Driver::Brave,
                search::Credentials {
                    key: plugin::settings::SecretUpdate::Replace(Secret::new(
                        "isolated-search-credential".into(),
                    )),
                    basic: plugin::settings::SecretUpdate::Keep,
                },
            )]),
        }),
    )
    .await
    else {
        panic!("search settings expected");
    };
    (code, search)
}

#[tokio::test]
async fn skips_same_and_older_bundled_versions() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let equal = version(&client, "code").await;
        let current = semver::Version::parse(&version(&client, "browser").await).unwrap();
        let newer = semver::Version::new(
            current.major,
            current.minor,
            current.patch.checked_add(1).unwrap(),
        );
        assert!(newer.cmp_precedence(&current).is_gt());
        let code = previous(directory.path(), &client, worktree, "code", &equal).await;
        let browser = previous(
            directory.path(),
            &client,
            worktree,
            "browser",
            &newer.to_string(),
        )
        .await;
        let code = disable(&client, code).await;
        let browser = disable(&client, browser).await;
        let captures = [code.summary.reference(), browser.summary.reference()];
        let mut captured = Vec::new();
        for package in &captures {
            captured.push(info(
                execute(
                    &client,
                    Command::ReadPluginVersion {
                        package: package.clone(),
                    },
                )
                .await,
            ));
        }
        let profile = directory.path().join("node");
        let path = profile.join("storage/node.turso");
        node.shutdown().await.unwrap();
        let previous = [bundled(&path, code).await, bundled(&path, browser).await];
        let retained = {
            let (handle, db) = crate::plugins::database_fixture::open(&path, true).await;
            let value = records(&db).await.unwrap();
            crate::plugins::database_fixture::close(handle, db).await;
            value
        };
        let assets: Vec<_> = previous
            .iter()
            .map(|info| {
                fs::read(
                    profile
                        .join("plugins/packages")
                        .join(&info.summary.digest)
                        .join("plugin.json"),
                )
                .unwrap()
            })
            .collect();
        let stored: std::collections::BTreeSet<_> = fs::read_dir(profile.join("plugins/packages"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        for ((previous, asset), captured) in previous.iter().zip(assets).zip(captured) {
            let current = info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: previous.summary.name.clone(),
                    },
                )
                .await,
            );
            assert!(
                current == *previous,
                "{:?} != {:?}",
                current.summary,
                previous.summary
            );
            assert_eq!(
                info(
                    execute(
                        &client,
                        Command::ReadPluginVersion {
                            package: captured.summary.reference(),
                        }
                    )
                    .await
                ),
                captured
            );
            assert_eq!(
                fs::read(
                    profile
                        .join("plugins/packages")
                        .join(&previous.summary.digest)
                        .join("plugin.json")
                )
                .unwrap(),
                asset
            );
        }
        assert_eq!(
            node.with_storage(|db| Box::pin(async move {
                records(db)
                    .await
                    .map_err(crate::plugins::database_fixture::fault)
            }))
            .await
            .unwrap(),
            retained
        );
        assert_eq!(
            fs::read_dir(profile.join("plugins/packages"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<std::collections::BTreeSet<_>>(),
            stored
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn retains_disabled_configuration_on_each_path() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let expected = BTreeMap::from([
            ("code", version(&client, "code").await),
            ("web-search", version(&client, "web-search").await),
        ]);
        let old = semver::Version::parse("0.1.0").unwrap();
        for version in expected.values() {
            assert!(
                semver::Version::parse(version)
                    .unwrap()
                    .cmp_precedence(&old)
                    .is_gt()
            );
        }
        let code = previous(directory.path(), &client, worktree, "code", "0.1.0").await;
        let search = previous(directory.path(), &client, worktree, "web-search", "0.1.0").await;
        let (session, turn) = capture(&client, worktree).await;
        let code = disable(&client, code).await;
        let search = disable(&client, search).await;
        let configured = settings(&client).await;
        let profile = directory.path().join("node");
        let path = profile.join("storage/node.turso");
        node.shutdown().await.unwrap();
        let previous = [bundled(&path, code).await, bundled(&path, search).await];
        let captures: Vec<u8> = {
            let (handle, db) = crate::plugins::database_fixture::open(&path, true).await;
            let value = db
                .prepare("SELECT plugins FROM turns WHERE id=?1")
                .await
                .unwrap()
                .query_row([turn.to_string()])
                .await
                .unwrap()
                .get::<Vec<u8>>(0)
                .unwrap();
            crate::plugins::database_fixture::close(handle, db).await;
            value
        };
        let packages: Vec<plugin::Reference> = serde_json::from_slice(&captures).unwrap();
        for previous in &previous {
            assert!(
                packages
                    .iter()
                    .any(|package| package.name == previous.summary.name
                        && package.digest == previous.summary.digest)
            );
        }
        let retained = {
            let (handle, db) = crate::plugins::database_fixture::open(&path, true).await;
            let value = records(&db).await.unwrap();
            crate::plugins::database_fixture::close(handle, db).await;
            value
        };
        let assets = previous
            .iter()
            .map(|previous| {
                fs::read(
                    profile
                        .join("plugins/packages")
                        .join(&previous.summary.digest)
                        .join("plugin.json"),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let secret: Vec<u8> = {
            let (handle, db) = crate::plugins::database_fixture::open(&path, true).await;
            let row = db
                .prepare("SELECT body FROM search_settings")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap();
            let value: turso::Result<_> = row.get(0);
            let value = value.unwrap();
            crate::plugins::database_fixture::close(handle, db).await;
            value
        };
        let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let mut refreshed = Vec::new();
        for (previous, asset) in previous.iter().zip(&assets) {
            let current = info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: previous.summary.name.clone(),
                    },
                )
                .await,
            );
            assert_eq!(
                current.summary.version.as_ref(),
                Some(&expected[previous.summary.name.as_str()])
            );
            assert_eq!(current.origin, Some(plugin::Origin::Bundled));
            assert_eq!(current.summary.revision, previous.summary.revision + 1);
            assert_eq!(current.summary.settings_revision, 1);
            assert!(!current.summary.enabled);
            assert_ne!(current.summary.digest, previous.summary.digest);
            let page = current
                .extension
                .as_ref()
                .unwrap()
                .settings_page
                .as_ref()
                .unwrap();
            assert_eq!(
                page.entry.as_deref(),
                Some("dev.sailry.platform/desktop/settings.js")
            );
            assert!(page.enabled_control);
            assert!(page.entry.is_some());
            let captured = info(
                execute(
                    &client,
                    Command::ReadPluginVersion {
                        package: packages
                            .iter()
                            .find(|package| {
                                package.name == previous.summary.name
                                    && package.digest == previous.summary.digest
                            })
                            .unwrap()
                            .clone(),
                    },
                )
                .await,
            );
            assert_eq!(captured.summary.digest, previous.summary.digest);
            assert_eq!(captured.summary.version, previous.summary.version);
            assert!(captured.extension.as_ref().unwrap().settings_page.is_none());
            assert_eq!(
                fs::read(
                    profile
                        .join("plugins/packages")
                        .join(&previous.summary.digest)
                        .join("plugin.json")
                )
                .unwrap(),
                *asset
            );
            refreshed.push(current);
        }
        assert_eq!(
            node.with_storage(|db| Box::pin(async move {
                records(db)
                    .await
                    .map_err(crate::plugins::database_fixture::fault)
            }))
            .await
            .unwrap(),
            retained
        );
        let Output::Session(current_session) = execute(
            &client,
            Command::ReadSession {
                session: session.id,
            },
        )
        .await
        else {
            panic!("session expected");
        };
        assert_eq!(current_session.config, session.config);
        assert_eq!(current_session.revision, session.revision);
        assert_eq!(current_session.roles, session.roles);
        let current_captures: Vec<u8> = {
            let parameters = turso::params![turn.to_string()];
            let sql = "SELECT plugins FROM turns WHERE id=?1";
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
        assert_eq!(current_captures, captures);
        assert_eq!(
            execute(&client, Command::ReadCodeSettings).await,
            Output::CodeSettings(configured.0.clone())
        );
        assert_eq!(
            execute(&client, Command::ReadSearchSettings).await,
            Output::SearchSettings(configured.1.clone())
        );
        let after: Vec<u8> = {
            let parameters = ();
            let sql = "SELECT body FROM search_settings";
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
        assert_eq!(after, secret);
        let inventory = execute(&client, Command::ListPlugins).await;
        node.shutdown().await.unwrap();
        let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(execute(&client, Command::ListPlugins).await, inventory);
        assert_eq!(
            execute(&client, Command::ReadCodeSettings).await,
            Output::CodeSettings(configured.0)
        );
        assert_eq!(
            execute(&client, Command::ReadSearchSettings).await,
            Output::SearchSettings(configured.1)
        );
        for current in refreshed {
            assert_eq!(
                info(
                    execute(
                        &client,
                        Command::ReadPlugin {
                            name: current.summary.name.clone()
                        }
                    )
                    .await
                ),
                current
            );
        }
        for (previous, asset) in previous.iter().zip(&assets) {
            assert_eq!(
                fs::read(
                    profile
                        .join("plugins/packages")
                        .join(&previous.summary.digest)
                        .join("plugin.json")
                )
                .unwrap(),
                *asset
            );
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn retains_browser_configuration_on_each_path() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let current = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "browser".into(),
                },
            )
            .await,
        );
        let version = current.summary.version.clone();
        let shipped_digest = current.summary.digest;
        let previous = previous(directory.path(), &client, worktree, "browser", "0.1.0").await;
        let previous = disable(&client, previous).await;
        let profile = directory.path().join("node");
        let path = profile.join("storage/node.turso");
        node.shutdown().await.unwrap();
        let previous = bundled(&path, previous).await;
        let retained = {
            let (handle, db) = crate::plugins::database_fixture::open(&path, true).await;
            let value = records(&db).await.unwrap();
            crate::plugins::database_fixture::close(handle, db).await;
            value
        };
        let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let current = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "browser".into(),
                },
            )
            .await,
        );
        assert_eq!(current.summary.version, version);
        assert_eq!(current.summary.digest, shipped_digest);
        assert_ne!(current.summary.digest, previous.summary.digest);
        assert_eq!(current.origin, Some(plugin::Origin::Bundled));
        assert_eq!(current.summary.revision, previous.summary.revision + 1);
        assert_eq!(
            current.summary.settings_revision,
            previous.summary.settings_revision
        );
        assert!(!current.summary.enabled);
        assert_eq!(
            node.with_storage(|db| Box::pin(async move {
                records(db)
                    .await
                    .map_err(crate::plugins::database_fixture::fault)
            }))
            .await
            .unwrap(),
            retained
        );
        let inventory = execute(&client, Command::ListPlugins).await;
        node.shutdown().await.unwrap();
        let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(execute(&client, Command::ListPlugins).await, inventory);
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "browser".into()
                    }
                )
                .await
            ),
            current
        );
        assert_eq!(
            node.with_storage(|db| Box::pin(async move {
                records(db)
                    .await
                    .map_err(crate::plugins::database_fixture::fault)
            }))
            .await
            .unwrap(),
            retained
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_removed_rows_on_each_path() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let code = previous(directory.path(), &client, worktree, "code", "0.1.0").await;
        let search = previous(directory.path(), &client, worktree, "web-search", "0.2.0").await;
        for installed in [code, search] {
            execute(
                &client,
                Command::RemovePlugin {
                    name: installed.summary.name,
                    expected_revision: installed.summary.revision,
                },
            )
            .await;
        }
        let inventory = execute(&client, Command::ListPlugins).await;
        let profile = directory.path().join("node");
        let path = profile.join("storage/node.turso");
        node.shutdown().await.unwrap();
        let (db_handle, db) = crate::plugins::database_fixture::open(&path, false).await;
        for name in ["code", "web-search"] {
            let body: Vec<u8> = (async {
                let mut statement = db.prepare("SELECT body FROM plugins WHERE name=?1").await?;
                let row = statement.query_row([name]).await?;
                let mapped: turso::Result<_> = row.get(0);
                mapped
            })
            .await
            .unwrap();
            let mut info: Info = serde_json::from_slice(&body).unwrap();
            info.origin = Some(plugin::Origin::Bundled);
            db.execute(
                "UPDATE plugins SET body=?1 WHERE name=?2",
                params![serde_json::to_vec(&info).unwrap(), name],
            )
            .await
            .unwrap();
        }
        let before: Vec<(String, Vec<u8>)> = {
            let mut rows = db
                .query(
                    "SELECT name,body FROM plugins WHERE installed=0 ORDER BY name",
                    (),
                )
                .await
                .unwrap();
            let mut values = Vec::new();
            while let Some(row) = rows.next().await.unwrap() {
                values.push((row.get(0).unwrap(), row.get(1).unwrap()));
            }
            values
        };
        crate::plugins::database_fixture::close(db_handle, db).await;
        let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(execute(&client, Command::ListPlugins).await, inventory);
        for name in ["code", "web-search"] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadPlugin { name: name.into() }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
        }

        let after: Vec<(String, Vec<u8>)> = node
            .with_storage(|db| {
                Box::pin(async move {
                    let mut rows = db
                        .query(
                            "SELECT name,body FROM plugins WHERE installed=0 ORDER BY name",
                            (),
                        )
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?;
                    let mut values = Vec::new();
                    while let Some(row) = rows
                        .next()
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?
                    {
                        values.push((
                            row.get::<String>(0)
                                .map_err(crate::plugins::database_fixture::fault)?,
                            row.get::<Vec<u8>>(1)
                                .map_err(crate::plugins::database_fixture::fault)?,
                        ));
                    }
                    Ok(values)
                })
            })
            .await
            .unwrap();
        assert_eq!(after, before);

        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
