use super::*;
use plugin::updates::{Batch, Candidate, Request as Selection};

async fn check(client: &Client, packages: &[Summary]) -> Batch {
    let request = client.prepare(Command::CheckPluginUpdates {
        force: true,
        packages: packages
            .iter()
            .map(|package| Selection {
                name: package.name.clone(),
                expected_revision: package.revision,
            })
            .collect(),
    });
    assert!(!request.command.durable());
    let admission = client.dispatch(request).await.unwrap();
    assert!(!admission.receipt.durable);
    let Output::PluginUpdates(batch) = admission.completion.await.unwrap().unwrap() else {
        panic!("plugin update reports expected");
    };
    batch
}

async fn installed(client: &Client) -> Vec<Summary> {
    let Output::Plugins(packages) = execute(client, Command::ListPlugins).await else {
        panic!("plugin inventory expected");
    };
    packages
}

fn stored(node: &Node) -> std::collections::BTreeSet<std::ffi::OsString> {
    fs::read_dir(node.profile().join("plugins/packages"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect()
}

async fn install_official(client: &Client, current: &Summary) -> Info {
    let Output::PluginSource(selected) = execute(
        client,
        Command::InspectPluginSource {
            source: plugin::skills::Source {
                repository: "https://github.com/sailry/sailry-plugins".into(),
                git_ref: Some("main".into()),
                path: Some(current.name.clone()),
            },
        },
    )
    .await
    else {
        panic!("immutable source selection expected");
    };
    info(
        execute(
            client,
            Command::InstallPluginSource {
                source: selected.source,
                path: selected.path,
                name: current.name.clone(),
                expected_revision: current.revision,
            },
        )
        .await,
    )
}

#[tokio::test]
async fn selects_the_highest_newer_version() {
    for remote in [false, true] {
        for available in ["0.0.2", "0.1.0", "0.2.0"] {
            let catalog = Catalog::with_versions("0.0.1", available, false).await;
            let (_directory, node, controller, client) = connection(remote, &catalog).await;
            let bundled = installed(&client)
                .await
                .into_iter()
                .find(|package| package.name == "reminders")
                .unwrap();
            let current = install_official(&client, &bundled).await;
            catalog.updated.store(true, Ordering::SeqCst);
            *catalog.version.lock().unwrap() = Some(available.into());
            catalog.requests.lock().unwrap().clear();
            let before = stored(&node);
            let batch = check(&client, std::slice::from_ref(&current.summary)).await;
            assert!(batch.failures.is_empty());
            match batch.reports[0].available.as_ref().unwrap() {
                Candidate::Package(info) => {
                    assert!(
                        semver::Version::parse(bundled.version.as_deref().unwrap())
                            .unwrap()
                            .cmp_precedence(&semver::Version::parse(available).unwrap())
                            .is_gt()
                    );
                    assert_eq!(info.summary.version, bundled.version);
                    assert_eq!(info.origin, Some(plugin::Origin::Bundled));
                }
                Candidate::Official(entry) => {
                    assert!(
                        !semver::Version::parse(bundled.version.as_deref().unwrap())
                            .unwrap()
                            .cmp_precedence(&semver::Version::parse(available).unwrap())
                            .is_gt()
                    );
                    assert_eq!(entry.version.as_deref(), Some(available));
                }
            }
            assert_eq!(*catalog.requests.lock().unwrap(), ["/catalog.json"]);
            assert_eq!(stored(&node), before);
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
}

#[tokio::test]
async fn skips_same_and_older_content_versions() {
    let version = fixture::bundled_version();
    let mut newer = semver::Version::parse(&version).unwrap();
    newer.minor += 1;
    newer.patch = 0;
    let newer = newer.to_string();
    for (remote, initial) in [
        (false, version.as_str()),
        (true, version.as_str()),
        (false, newer.as_str()),
        (true, newer.as_str()),
    ] {
        let catalog = Catalog::with_content_updates(initial).await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let bundled = installed(&client)
            .await
            .into_iter()
            .find(|package| package.name == "reminders")
            .unwrap();
        let installed = install_official(&client, &bundled).await;
        assert_eq!(installed.summary.version.as_deref(), Some(initial));
        assert!(matches!(
            &installed.origin,
            Some(plugin::Origin::Online { .. })
        ));
        assert_ne!(installed.summary.digest, bundled.digest);
        let current = info(
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
        let captured = info(
            execute(
                &client,
                Command::ReadPluginVersion {
                    package: current.summary.reference(),
                },
            )
            .await,
        );
        let before = stored(&node);
        for changed in [false, true] {
            catalog.updated.store(changed, Ordering::SeqCst);
            catalog.requests.lock().unwrap().clear();
            let batch = check(&client, std::slice::from_ref(&current.summary)).await;
            assert!(batch.failures.is_empty());
            assert!(batch.reports[0].available.is_none());
            assert_eq!(*catalog.requests.lock().unwrap(), ["/catalog.json"]);
            assert_eq!(stored(&node), before);
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
        }
        catalog.status.store(503, Ordering::SeqCst);
        let offline = check(&client, std::slice::from_ref(&current.summary)).await;
        assert!(offline.reports.is_empty());
        assert_eq!(offline.failures[0].error.code, ErrorCode::Unavailable);
        assert_eq!(stored(&node), before);
        catalog.status.store(200, Ordering::SeqCst);
        // Explicit reinstallation remains available; it is not update detection.
        let updated = install_official(&client, &current.summary).await;
        assert_eq!(updated.summary.version.as_deref(), Some(version.as_str()));
        assert_ne!(updated.summary.digest, current.summary.digest);
        assert!(!updated.summary.enabled);
        assert_eq!(
            updated.summary.settings_revision,
            current.summary.settings_revision
        );
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPluginVersion {
                        package: current.summary.reference(),
                    }
                )
                .await
            ),
            captured
        );
        let latest = check(&client, std::slice::from_ref(&updated.summary)).await;
        assert!(latest.failures.is_empty());
        assert!(latest.reports[0].available.is_none());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn skips_same_version_bundled_content() {
    let version = fixture::bundled_version();
    for remote in [false, true] {
        let catalog = Catalog::with_content_updates(&version).await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let bundled = installed(&client)
            .await
            .into_iter()
            .find(|package| package.name == "reminders")
            .unwrap();
        let mut current = install_official(&client, &bundled).await;
        assert_eq!(current.summary.version, bundled.version);
        assert_ne!(current.summary.digest, bundled.digest);
        current.origin = Some(plugin::Origin::Bundled);
        let database =
            rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
        database
            .execute(
                "UPDATE plugins SET body=?1 WHERE name='reminders'",
                [serde_json::to_vec(&current).unwrap()],
            )
            .unwrap();
        let before = stored(&node);
        catalog.requests.lock().unwrap().clear();
        let batch = check(&client, std::slice::from_ref(&current.summary)).await;
        assert!(batch.failures.is_empty());
        assert!(batch.reports[0].available.is_none());
        assert_eq!(*catalog.requests.lock().unwrap(), ["/catalog.json"]);
        catalog.status.store(503, Ordering::SeqCst);
        let offline = check(&client, std::slice::from_ref(&current.summary)).await;
        assert!(offline.reports.is_empty());
        assert_eq!(offline.failures[0].error.code, ErrorCode::Unavailable);
        assert_eq!(stored(&node), before);
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
async fn offers_bundled_updates_on_catalog_failure() {
    for (remote, initial) in [
        (false, "0.0.1"),
        (true, "0.0.1"),
        (false, "0.0.9"),
        (true, "0.0.9"),
    ] {
        let catalog = Catalog::with_mcp_reminders(initial).await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let mut events = client.subscribe().await.unwrap();
        const TOKEN: &str = "Bearer synthetic-bundled-private-token";
        assert!(
            !serde_json::to_string(&events.next().await.unwrap())
                .unwrap()
                .contains(TOKEN)
        );
        let bundled = installed(&client)
            .await
            .into_iter()
            .find(|package| package.name == "reminders")
            .unwrap();
        let Output::PluginSource(selected) = execute(
            &client,
            Command::InspectPluginSource {
                source: plugin::skills::Source {
                    repository: "https://github.com/sailry/sailry-plugins".into(),
                    git_ref: Some("main".into()),
                    path: Some("reminders".into()),
                },
            },
        )
        .await
        else {
            panic!("source expected");
        };
        let old = info(
            execute(
                &client,
                Command::InstallPluginSource {
                    source: selected.source,
                    path: selected.path,
                    name: "reminders".into(),
                    expected_revision: bundled.revision,
                },
            )
            .await,
        );
        assert_eq!(old.summary.version.as_deref(), Some(initial));
        let Output::PluginMcp(mut configured) = execute(
            &client,
            Command::ReadPluginMcp {
                package: old.summary.reference(),
            },
        )
        .await
        else {
            panic!("MCP settings expected")
        };
        let plugin::mcp::Server::Http { url, headers } =
            configured.configuration.servers.get_mut("service").unwrap()
        else {
            unreachable!()
        };
        *url = "http://127.0.0.1:9/saved-mcp".into();
        headers.insert("Authorization".into(), TOKEN.into());
        let configured = info(
            execute(
                &client,
                Command::SavePluginMcp {
                    package: configured.package,
                    configuration: configured.configuration,
                },
            )
            .await,
        );
        assert_eq!(configured.summary.settings_revision, 1);
        let current = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "reminders".into(),
                    expected_revision: configured.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        let database =
            rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
        let private = || {
            database
                .query_row(
                    "SELECT body FROM plugin_settings WHERE plugin='reminders' AND revision=?1",
                    [current.summary.settings_revision as i64],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .unwrap()
        };
        let body = private();
        let saved: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            saved["mcp"]["mcpServers"]["service"]["url"],
            "http://127.0.0.1:9/saved-mcp"
        );
        assert_eq!(
            saved["mcp"]["mcpServers"]["service"]["headers"]["Authorization"],
            TOKEN
        );
        let captured = info(
            execute(
                &client,
                Command::ReadPluginVersion {
                    package: current.summary.reference(),
                },
            )
            .await,
        );
        catalog.updated.store(true, Ordering::SeqCst);
        let online = check(&client, std::slice::from_ref(&current.summary)).await;
        assert!(online.failures.is_empty());
        assert!(matches!(
            online.reports[0].available,
            Some(Candidate::Official(_))
        ));

        catalog.status.store(503, Ordering::SeqCst);
        let before = stored(&node);
        let offline = check(&client, std::slice::from_ref(&current.summary)).await;
        assert!(offline.failures.is_empty());
        let Some(Candidate::Package(candidate)) = &offline.reports[0].available else {
            panic!("bundled update expected");
        };
        assert_eq!(candidate.origin, Some(plugin::Origin::Bundled));
        assert_eq!(candidate.summary.version, bundled.version);
        assert_eq!(candidate.summary.digest, bundled.digest);
        assert_eq!(stored(&node), before);
        assert_eq!(private(), body);
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

        catalog.status.store(200, Ordering::SeqCst);
        for (invalid_index, obsolete, version) in [
            (true, false, None),
            (false, true, None),
            (false, false, Some("invalid")),
        ] {
            catalog.invalid.store(invalid_index, Ordering::SeqCst);
            catalog.obsolete.store(obsolete, Ordering::SeqCst);
            *catalog.version.lock().unwrap() = version.map(str::to_owned);
            catalog.requests.lock().unwrap().clear();
            let invalid = check(&client, std::slice::from_ref(&current.summary)).await;
            assert!(invalid.failures.is_empty());
            assert_eq!(invalid.reports[0].available, offline.reports[0].available);
            assert_eq!(*catalog.requests.lock().unwrap(), ["/catalog.json"]);
            assert_eq!(stored(&node), before);
            assert_eq!(private(), body);
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
        }

        let updated = info(
            execute(
                &client,
                Command::InstallBundledPlugin {
                    name: "reminders".into(),
                    expected_revision: current.summary.revision,
                },
            )
            .await,
        );
        assert_eq!(updated.summary.digest, bundled.digest);
        assert_eq!(updated.summary.version, bundled.version);
        assert_eq!(updated.origin, Some(plugin::Origin::Bundled));
        assert!(!updated.summary.enabled);
        assert_eq!(
            updated.summary.settings_revision,
            current.summary.settings_revision
        );
        assert_eq!(private(), body);
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPluginVersion {
                        package: current.summary.reference(),
                    },
                )
                .await,
            ),
            captured
        );
        catalog.obsolete.store(true, Ordering::SeqCst);
        *catalog.version.lock().unwrap() = None;
        let invalid = check(&client, std::slice::from_ref(&updated.summary)).await;
        assert!(invalid.reports.is_empty());
        assert_eq!(invalid.failures.len(), 1);
        assert_eq!(invalid.failures[0].error.code, ErrorCode::InvalidRequest);
        assert_eq!(
            invalid.failures[0].error.message,
            "invalid official plugin identity or manifest"
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = events.next().await.unwrap();
                assert!(!serde_json::to_string(&event).unwrap().contains(TOKEN));
                if let Update::Event(envelope) = event
                    && matches!(
                        &envelope.event,
                        Event::PluginChanged(summary) if summary.name == "reminders"
                            && summary.revision == updated.summary.revision
                    )
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        catalog.invalid.store(false, Ordering::SeqCst);
        catalog.obsolete.store(false, Ordering::SeqCst);
        catalog.updated.store(false, Ordering::SeqCst);
        let final_check = check(&client, std::slice::from_ref(&updated.summary)).await;
        assert!(final_check.failures.is_empty());
        assert!(final_check.reports[0].available.is_none());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn checks_one_index_without_installing_packages() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        catalog.updated.store(true, Ordering::SeqCst);
        let bundled = installed(&client)
            .await
            .into_iter()
            .find(|package| package.name == "files")
            .unwrap();
        let online = install_official(&client, &bundled).await;
        assert_eq!(online.summary.digest, bundled.digest);
        assert!(matches!(
            &online.origin,
            Some(plugin::Origin::Online { .. })
        ));
        let packages = installed(&client).await;
        let before = stored(&node);
        let Output::PluginCatalog(page) = execute(&client, search("reminders")).await else {
            panic!("official catalog expected");
        };
        let listing = page
            .entries
            .into_iter()
            .find(|entry| entry.id == "reminders")
            .unwrap();
        catalog.requests.lock().unwrap().clear();
        let batch = check(&client, &packages).await;
        assert!(batch.failures.is_empty(), "{:?}", batch.failures);
        assert_eq!(batch.reports.len(), packages.len());
        assert_eq!(*catalog.requests.lock().unwrap(), ["/catalog.json"]);
        for report in batch.reports {
            assert!(packages.contains(&report.current));
            assert!(!report.manual);
            if report.current.name == "reminders" {
                assert_eq!(
                    report.available,
                    Some(Candidate::Official(Box::new(listing.clone())))
                );
            } else {
                assert!(
                    report.available.is_none(),
                    "unexpected update for {} (installed digest {}): {:?}",
                    report.current.name,
                    report.current.digest,
                    report.available
                );
            }
        }
        assert_eq!(installed(&client).await, packages);
        assert_eq!(stored(&node), before);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn reports_offline_and_invalid_indexes() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let packages = installed(&client).await;
        for (status, invalid, expected) in [
            (503, false, ErrorCode::Unavailable),
            (200, true, ErrorCode::InvalidRequest),
        ] {
            catalog.status.store(status, Ordering::SeqCst);
            catalog.invalid.store(invalid, Ordering::SeqCst);
            catalog.requests.lock().unwrap().clear();
            let batch = check(&client, &packages).await;
            assert!(batch.reports.is_empty());
            assert_eq!(batch.failures.len(), packages.len());
            assert!(
                batch
                    .failures
                    .iter()
                    .all(|failure| failure.error.code == expected)
            );
            assert_eq!(*catalog.requests.lock().unwrap(), ["/catalog.json"]);
            assert_eq!(installed(&client).await, packages);
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn checks_semantic_catalog_versions() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let current = installed(&client)
            .await
            .into_iter()
            .find(|package| package.name == "reminders")
            .unwrap();
        for (version, expected) in [
            ("0.1.0", Some(false)),
            ("0.0.9", Some(false)),
            ("0.1.0+build", Some(false)),
            ("0.1.0-rc.1", Some(false)),
            ("0.2.0", Some(true)),
            ("", None),
            ("invalid", None),
        ] {
            *catalog.version.lock().unwrap() = Some(version.into());
            catalog.requests.lock().unwrap().clear();
            let batch = check(&client, std::slice::from_ref(&current)).await;
            match expected {
                Some(available) => {
                    assert!(batch.failures.is_empty());
                    assert_eq!(batch.reports.len(), 1);
                    assert_eq!(batch.reports[0].current, current);
                    assert_eq!(batch.reports[0].available.is_some(), available);
                }
                None => {
                    assert!(batch.reports.is_empty());
                    assert_eq!(batch.failures.len(), 1);
                    assert_eq!(batch.failures[0].error.code, ErrorCode::InvalidRequest);
                }
            }
            assert_eq!(*catalog.requests.lock().unwrap(), ["/catalog.json"]);
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn keeps_local_checks_when_official_fetch_fails() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (directory, node, controller, client) = connection(remote, &catalog).await;
        let root = directory.path().join("project");
        package(&root.join("package"), "1.0.0");
        execute(
            &client,
            Command::RegisterProject {
                name: "Update fixture".into(),
                path: root.to_str().unwrap().into(),
            },
        )
        .await;
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        let current = info(execute(&client, install(snapshot.worktrees[0].id, 0)).await);
        let mut packages = installed(&client).await;
        packages.retain(|package| matches!(package.name.as_str(), "reminders" | "example"));
        fs::write(
            root.join("package/skills/analysis/references/guide.md"),
            "Same version, new content",
        )
        .unwrap();
        package(&root.join("package"), "1.0.1");
        catalog.status.store(503, Ordering::SeqCst);
        let batch = check(&client, &packages).await;
        assert_eq!(batch.failures.len(), 1);
        assert_eq!(batch.failures[0].name, "reminders");
        assert_eq!(batch.failures[0].error.code, ErrorCode::Unavailable);
        assert_eq!(batch.reports.len(), 1);
        let report = &batch.reports[0];
        assert_eq!(report.current, current.summary);
        let Some(Candidate::Package(candidate)) = &report.available else {
            panic!("inspected local package expected");
        };
        assert_eq!(candidate.summary.version.as_deref(), Some("1.0.1"));
        assert_ne!(candidate.summary.digest, current.summary.digest);
        assert_eq!(*catalog.requests.lock().unwrap(), ["/catalog.json"]);
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "example".into()
                    }
                )
                .await
            ),
            current
        );
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: current.summary.revision,
                enabled: false,
            },
        )
        .await;
        catalog.requests.lock().unwrap().clear();
        let stale = check(&client, &[current.summary]).await;
        assert!(stale.reports.is_empty());
        assert_eq!(stale.failures[0].error.code, ErrorCode::RevisionConflict);
        assert!(catalog.requests.lock().unwrap().is_empty());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_invalid_selections_without_fetching() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let current = installed(&client).await.remove(0);
        let selection = Selection {
            name: current.name,
            expected_revision: current.revision,
        };
        for packages in [
            vec![selection.clone(); 2],
            vec![selection; plugin::MAX_INSTALLED + 1],
        ] {
            let error = client
                .execute(client.prepare(Command::CheckPluginUpdates {
                    packages,
                    force: true,
                }))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert!(catalog.requests.lock().unwrap().is_empty());
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
